//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! Admin-only management UI for the agent-friendly API keys introduced
//! by issue #1957.
//!
//! Three operations: create, list, revoke. The create dialog is the
//! security-critical surface — it shows the raw key material exactly
//! once, with explicit copy-and-confirm UX, and never re-renders it
//! after the user dismisses the dialog. The daemon only stores a
//! SHA-256 hash, so there is no recoverable copy if the user loses
//! the key.

import { useCreateApiKey, useListApiKeys, useRevokeApiKey } from "@api/hooks/useApiKeys";
import CopyToClipboard from "@components/CopyToClipboard";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { StyledPaper } from "@components/StyledComponents";
import type { SelectChangeEvent } from "@mui/material";
import {
  Alert,
  Box,
  Button,
  Checkbox,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  FormControl,
  FormControlLabel,
  InputLabel,
  ListItemText,
  MenuItem,
  OutlinedInput,
  Select,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from "@mui/material";
import { permissionToString, type AuthCreateApiKeyResponse, type IssuedApiKey } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";

/// Permissions the create-key dialog offers as selectable checkboxes.
///
/// Grants are encoded as `<resource>:<action>[:<entity>]` strings (lowercase,
/// snake_case for two-word resources). The picker here surfaces the
/// unscoped forms only — narrowing to a specific account/resource/NFT
/// would need an entity input which is out of scope for this iteration.
/// The daemon's `Permissions::from_str` accepts every entry below verbatim.
///
/// `admin` is gated by a separate explicit confirmation checkbox below the
/// list because granting it to a long-lived agent key is high-risk.
const PERMISSION_OPTIONS: Array<{ value: string; label: string; description: string }> = [
  {
    value: "accounts:read",
    label: "accounts:read",
    description: "Read account metadata and list accounts.",
  },
  {
    value: "accounts:create",
    label: "accounts:create",
    description: "Create new accounts.",
  },
  {
    value: "accounts:update",
    label: "accounts:update",
    description: "Rename accounts, set the default, associate stealth resources.",
  },
  {
    value: "keys:read",
    label: "keys:read",
    description: "Enumerate keys held by the key manager.",
  },
  {
    value: "keys:create",
    label: "keys:create",
    description: "Mint new keys.",
  },
  {
    value: "transactions:read",
    label: "transactions:read",
    description: "Read transaction history and dry-run results.",
  },
  {
    value: "transactions:create",
    label: "transactions:create",
    description: "Submit arbitrary transactions, including raw instructions and manifests.",
  },
  {
    value: "transfer:create",
    label: "transfer:create",
    description: "Submit transfers (fund, NFT, stealth, confidential, burn-claim) from any account.",
  },
  {
    value: "templates:read",
    label: "templates:read",
    description: "Read deployed contract templates.",
  },
  {
    value: "templates:create",
    label: "templates:create",
    description: "Publish templates and sign template metadata.",
  },
  {
    value: "nfts:read",
    label: "nfts:read",
    description: "Read NFT data the wallet holds.",
  },
  {
    value: "confidential:read",
    label: "confidential:read",
    description: "View confidential vault balances.",
  },
  {
    value: "confidential:create",
    label: "confidential:create",
    description: "Generate confidential transfer / output proofs.",
  },
  {
    value: "stealth_utxos:read",
    label: "stealth_utxos:read",
    description: "List and decrypt stealth UTXOs.",
  },
  {
    value: "validators:read",
    label: "validators:read",
    description: "Read validator fee pools.",
  },
  {
    value: "validators:update",
    label: "validators:update",
    description: "Claim validator fees.",
  },
  {
    value: "address_book:read",
    label: "address_book:read",
    description: "Read address book entries.",
  },
  {
    value: "address_book:create",
    label: "address_book:create",
    description: "Add address book entries.",
  },
  {
    value: "address_book:update",
    label: "address_book:update",
    description: "Edit address book entries.",
  },
  {
    value: "address_book:delete",
    label: "address_book:delete",
    description: "Remove address book entries.",
  },
  {
    value: "settings:read",
    label: "settings:read",
    description: "Read wallet daemon settings.",
  },
  {
    value: "settings:update",
    label: "settings:update",
    description: "Modify wallet daemon settings.",
  },
  {
    value: "substates:read",
    label: "substates:read",
    description: "Read on-chain substate data.",
  },
  {
    value: "burn_proofs:read",
    label: "burn_proofs:read",
    description: "Read burn proofs known to the wallet.",
  },
  {
    value: "swap_pools:read",
    label: "swap_pools:read",
    description: "Read swap pool state and exchange rates.",
  },
];

const ADMIN_PERMISSION_VALUE = "admin";

/// Expiry radio choices. Presets are duration-relative (applied at submit
/// time, not dialog-open time) so the wire timestamp reflects when the
/// admin clicked Create. `custom` reveals a date picker; `never` maps to
/// `null` on the wire.
type ExpiryChoice = "5m" | "1h" | "24h" | "5d" | "custom" | "never";

/// Preset durations in seconds, paired with their compact button label.
/// Order matches the button-group render so changing this list updates
/// the UI in place.
const EXPIRY_PRESETS: Array<{ value: ExpiryChoice; label: string; seconds: number }> = [
  { value: "5m", label: "5m", seconds: 5 * 60 },
  { value: "1h", label: "1h", seconds: 60 * 60 },
  { value: "24h", label: "24h", seconds: 24 * 60 * 60 },
  { value: "5d", label: "5d", seconds: 5 * 24 * 60 * 60 },
];

/// Render a unix-second timestamp as a locale string. `null` -> "never".
function fmtTs(ts: bigint | null | undefined): string {
  if (ts == null) {
    return "never";
  }
  return new Date(Number(ts) * 1000).toLocaleString();
}

export default function ApiKeys() {
  // The "Show revoked" toggle defaults off — the typical admin view is
  // the currently-usable set. Flipping it refetches against the daemon
  // because `useListApiKeys` includes the flag in its query key.
  const [includeRevoked, setIncludeRevoked] = useState(false);
  const { data, isLoading, error, isError } = useListApiKeys(includeRevoked);
  const { mutate: revoke } = useRevokeApiKey();

  // Stash the create-result here so the dialog can show the raw key
  // exactly once. Set back to null on dialog close — the key string
  // is never persisted in component state across re-renders beyond
  // that dialog's lifetime.
  const [createdKey, setCreatedKey] = useState<AuthCreateApiKeyResponse | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createName, setCreateName] = useState("");
  // Tracked as a Set for O(1) toggle + membership checks. We convert to an
  // ordered Array on submit so the daemon sees them in PERMISSION_OPTIONS order.
  const [selectedPerms, setSelectedPerms] = useState<Set<string>>(() => new Set());
  const [grantAdmin, setGrantAdmin] = useState(false);
  const [confirmAdmin, setConfirmAdmin] = useState(false);
  // Expiry as a radio choice. Presets ("1h" / "5h" / "24h" / "5d") encode
  // a duration applied at submit time so the absolute timestamp matches the
  // instant the user clicks Create — not the moment they opened the dialog.
  // "custom" reveals a date picker; "never" sends `null` on the wire.
  const [expiryChoice, setExpiryChoice] = useState<ExpiryChoice>("never");
  const [customExpiresOn, setCustomExpiresOn] = useState<string>("");
  const [createError, setCreateError] = useState<string | null>(null);

  const resetCreateForm = () => {
    setCreateName("");
    setSelectedPerms(new Set());
    setGrantAdmin(false);
    setConfirmAdmin(false);
    setExpiryChoice("never");
    setCustomExpiresOn("");
    setCreateError(null);
  };

  const { mutate: createKey, isPending: isCreating } = useCreateApiKey((response) => {
    setCreatedKey(response);
    setCreateOpen(false);
    resetCreateForm();
  });

  const handleCreate = () => {
    setCreateError(null);
    // Build the permission list in PERMISSION_OPTIONS order so the wire
    // payload is stable regardless of click order. Then append Admin last
    // if granted (and gated by the explicit confirmation checkbox).
    const permissions = PERMISSION_OPTIONS.filter((opt) => selectedPerms.has(opt.value)).map((opt) => opt.value);
    if (grantAdmin) {
      permissions.push(ADMIN_PERMISSION_VALUE);
    }
    if (permissions.length === 0) {
      setCreateError("Select at least one permission; an unusable key is refused by the daemon.");
      return;
    }
    if (grantAdmin && !confirmAdmin) {
      setCreateError("Granting the Admin permission requires explicit confirmation. Tick the box below to proceed.");
      return;
    }
    // Resolve the expiry radio choice to a unix-seconds wire value. Presets
    // are applied at submit time (now + duration) so the timestamp matches
    // the instant of click; "custom" parses a local-tz YYYY-MM-DD picked
    // by the user (end-of-day in that tz); "never" leaves it null.
    let expires_at: bigint | null = null;
    const preset = EXPIRY_PRESETS.find((p) => p.value === expiryChoice);
    if (preset) {
      const ts = Math.floor(Date.now() / 1000) + preset.seconds;
      expires_at = BigInt(ts);
    } else if (expiryChoice === "custom") {
      if (!customExpiresOn) {
        setCreateError("Pick an expiry date, or choose 'Never expires'.");
        return;
      }
      const [y, m, d] = customExpiresOn.split("-").map((s) => parseInt(s, 10));
      if (!y || !m || !d) {
        setCreateError("Expiry date must be a valid calendar date.");
        return;
      }
      // End-of-day in local time is the friendlier reading of "expires on
      // this date" — the key stays usable through the whole of that day
      // rather than dying at midnight at the start.
      const localEndOfDay = new Date(y, m - 1, d, 23, 59, 59);
      const ts = Math.floor(localEndOfDay.getTime() / 1000);
      if (ts <= Math.floor(Date.now() / 1000)) {
        setCreateError("Expiry must be in the future.");
        return;
      }
      expires_at = BigInt(ts);
    }
    // expiryChoice === "never" → leave expires_at null
    createKey(
      {
        name: createName,
        permissions,
        confirm_admin: grantAdmin && confirmAdmin,
        expires_at,
      },
      {
        onError: (err: unknown) => {
          setCreateError(err instanceof Error ? err.message : "Create failed");
        },
      },
    );
  };

  /// Tomorrow's date in YYYY-MM-DD as the minimum the date picker accepts —
  /// the daemon rejects past expiries server-side anyway, but pre-validating
  /// here keeps the UX honest.
  const minExpiryDate = (() => {
    const t = new Date();
    t.setDate(t.getDate() + 1);
    const y = t.getFullYear();
    const m = String(t.getMonth() + 1).padStart(2, "0");
    const d = String(t.getDate()).padStart(2, "0");
    return `${y}-${m}-${d}`;
  })();

  return (
    <Stack spacing={2}>
      <Alert severity="info">
        API keys are long-lived credentials used by AI agents and other automated clients. They are bound to the
        permission scopes you choose at creation time.
      </Alert>

      <Stack direction="row" justifyContent="space-between" alignItems="center">
        <FormControlLabel
          control={
            <Checkbox size="small" checked={includeRevoked} onChange={(e) => setIncludeRevoked(e.target.checked)} />
          }
          label={<Typography variant="body2">Show revoked</Typography>}
        />
        <Button variant="contained" onClick={() => setCreateOpen(true)}>
          Create API Key
        </Button>
      </Stack>

      <FetchStatusCheck
        errorMessage={error?.message || "Error fetching API keys"}
        isError={isError}
        isLoading={isLoading}
      >
        <StyledPaper>
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>ID</TableCell>
                  <TableCell>Name</TableCell>
                  <TableCell>Permissions</TableCell>
                  <TableCell>Created</TableCell>
                  <TableCell>Last used</TableCell>
                  <TableCell>Expires</TableCell>
                  <TableCell>Status</TableCell>
                  <TableCell align="right">Actions</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {(data?.keys || []).map((k: IssuedApiKey) => {
                  const revoked = k.revoked_at != null;
                  // Treat the row as expired only when there's an expiry
                  // timestamp and it lies in the past. Revoked takes
                  // precedence in the status pill (a revoked key that was
                  // also going to expire is still presented as REVOKED).
                  const expired = !revoked && k.expires_at != null && Number(k.expires_at) * 1000 <= Date.now();
                  const statusLabel = revoked ? "REVOKED" : expired ? "EXPIRED" : "active";
                  const statusColor: "default" | "warning" | "success" = revoked
                    ? "default"
                    : expired
                      ? "warning"
                      : "success";
                  return (
                    <TableRow key={k.id}>
                      <TableCell>{k.id}</TableCell>
                      <TableCell>{k.name}</TableCell>
                      <TableCell>
                        <Stack direction="row" spacing={0.5} flexWrap="wrap">
                          {k.permissions.map((p, idx) => (
                            <Chip key={idx} label={permissionToString(p)} size="small" />
                          ))}
                        </Stack>
                      </TableCell>
                      <TableCell>{fmtTs(k.created_at)}</TableCell>
                      <TableCell>{fmtTs(k.last_used_at)}</TableCell>
                      <TableCell>{k.expires_at == null ? "never" : fmtTs(k.expires_at)}</TableCell>
                      <TableCell>
                        <Chip label={statusLabel} color={statusColor} size="small" />
                      </TableCell>
                      <TableCell align="right">
                        <Button variant="outlined" size="small" disabled={revoked} onClick={() => revoke({ id: k.id })}>
                          Revoke
                        </Button>
                      </TableCell>
                    </TableRow>
                  );
                })}
                {(data?.keys || []).length === 0 && (
                  <TableRow>
                    <TableCell colSpan={8} align="center">
                      No API keys issued.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </TableContainer>
        </StyledPaper>
      </FetchStatusCheck>

      {/* Create dialog */}
      <Dialog
        open={createOpen}
        onClose={() => {
          setCreateOpen(false);
          resetCreateForm();
        }}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>Create API Key</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1 }}>
            <TextField
              label="Name"
              value={createName}
              onChange={(e) => setCreateName(e.target.value)}
              helperText="A friendly label for this key. Not unique."
              fullWidth
            />
            <Box>
              <Typography variant="subtitle2" sx={{ mb: 0.5 }}>
                Permissions
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
                Pick the smallest set the agent needs. Each entry maps to one variant of the daemon's{" "}
                <code>Permissions</code> set.
              </Typography>
              <FormControl fullWidth size="small">
                <InputLabel id="api-key-permissions-label">Permissions</InputLabel>
                <Select<string[]>
                  labelId="api-key-permissions-label"
                  multiple
                  value={Array.from(selectedPerms)}
                  onChange={(event: SelectChangeEvent<string[]>) => {
                    // MUI types `target.value` as `string | string[]` even
                    // with `multiple`. The string case only happens if the
                    // <select> is rendered as a native form control (it's
                    // not, here), but we handle it for type-safety.
                    const value = event.target.value;
                    const next: string[] = Array.isArray(value) ? value : value.split(",");
                    setSelectedPerms(new Set(next));
                  }}
                  input={<OutlinedInput label="Permissions" />}
                  renderValue={(selected: string[]) =>
                    selected.length === 0 ? (
                      <Typography variant="body2" color="text.secondary">
                        None selected
                      </Typography>
                    ) : (
                      <Box sx={{ display: "flex", flexWrap: "wrap", gap: 0.5 }}>
                        {selected.map((value: string) => (
                          <Chip key={value} label={value} size="small" />
                        ))}
                      </Box>
                    )
                  }
                  // Cap dropdown height so long permission lists stay scrollable
                  // instead of pushing the dialog past the viewport.
                  MenuProps={{
                    PaperProps: { sx: { maxHeight: 320 } },
                  }}
                >
                  {PERMISSION_OPTIONS.map((opt) => (
                    <MenuItem key={opt.value} value={opt.value}>
                      <Checkbox size="small" checked={selectedPerms.has(opt.value)} />
                      <ListItemText
                        primary={opt.label}
                        secondary={opt.description}
                        primaryTypographyProps={{ variant: "body2" }}
                        secondaryTypographyProps={{ variant: "caption" }}
                      />
                    </MenuItem>
                  ))}
                </Select>
              </FormControl>
              <Divider sx={{ my: 1.5 }} />
              <FormControlLabel
                sx={{ alignItems: "flex-start", mr: 0 }}
                control={
                  <Checkbox
                    size="small"
                    color="warning"
                    sx={{ pt: 0.5 }}
                    checked={grantAdmin}
                    onChange={(e) => {
                      setGrantAdmin(e.target.checked);
                      if (!e.target.checked) {
                        setConfirmAdmin(false);
                      }
                    }}
                  />
                }
                label={
                  <Box>
                    <Typography variant="body2" component="span" color="warning.main">
                      Admin
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block" }}>
                      Full daemon access including account creation, key management, and module registration. Avoid
                      granting to a long-lived agent key unless absolutely required.
                    </Typography>
                  </Box>
                }
              />
              {grantAdmin && (
                <FormControlLabel
                  sx={{ alignItems: "flex-start", ml: 4, mt: 0.5 }}
                  control={
                    <Checkbox
                      size="small"
                      sx={{ pt: 0.5 }}
                      checked={confirmAdmin}
                      onChange={(e) => setConfirmAdmin(e.target.checked)}
                    />
                  }
                  label={
                    <Typography variant="body2">
                      I understand granting Admin gives this agent full control of the wallet daemon.
                    </Typography>
                  }
                />
              )}
            </Box>
            <Divider />
            <Box>
              <Typography variant="subtitle2" sx={{ mb: 0.5 }}>
                Expiry
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
                Shorter is safer — the daemon stops accepting the key the moment its expiry passes.
              </Typography>
              <ToggleButtonGroup
                value={expiryChoice}
                exclusive
                size="small"
                color="primary"
                onChange={(_event, value: ExpiryChoice | null) => {
                  // `exclusive` lets the group's value be `null` when the
                  // user clicks the currently-selected button (toggling it
                  // off). We don't want a null state — re-selecting the
                  // same button is a no-op.
                  if (value !== null) {
                    setExpiryChoice(value);
                  }
                }}
                aria-label="API key expiry"
                sx={{ flexWrap: "wrap", gap: 0.5 }}
              >
                {EXPIRY_PRESETS.map((p) => (
                  <ToggleButton key={p.value} value={p.value} aria-label={p.label}>
                    {p.label}
                  </ToggleButton>
                ))}
                <ToggleButton value="custom">Custom</ToggleButton>
                <ToggleButton value="never">Never</ToggleButton>
              </ToggleButtonGroup>
              {expiryChoice === "custom" && (
                <TextField
                  type="date"
                  label="Expires on"
                  value={customExpiresOn}
                  onChange={(e) => setCustomExpiresOn(e.target.value)}
                  InputLabelProps={{ shrink: true }}
                  inputProps={{ min: minExpiryDate }}
                  helperText="Key stops working at end of day in your local time."
                  fullWidth
                  size="small"
                  sx={{ mt: 1 }}
                />
              )}
            </Box>
            {createError && <Alert severity="error">{createError}</Alert>}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button
            onClick={() => {
              setCreateOpen(false);
              resetCreateForm();
            }}
          >
            Cancel
          </Button>
          <Button
            onClick={handleCreate}
            variant="contained"
            disabled={isCreating || createName.trim().length === 0 || (selectedPerms.size === 0 && !grantAdmin)}
          >
            Create
          </Button>
        </DialogActions>
      </Dialog>

      {/* "Here's your key, exactly once" dialog. Only rendered when
          createdKey is set; closing it nulls the state so the raw key
          is no longer in the component tree. */}
      <Dialog
        open={createdKey != null}
        onClose={() => setCreatedKey(null)}
        maxWidth="sm"
        fullWidth
        // The "click outside to close" affordance is intentional — but
        // the user is responsible for copying the key before dismissing.
      >
        <DialogTitle>API Key Created</DialogTitle>
        <DialogContent>
          <DialogContentText sx={{ mb: 2 }}>
            <strong>This is the only time the raw key will be shown.</strong> Copy it now and store it in a secrets
            manager.
          </DialogContentText>
          {createdKey && (
            <Stack spacing={1}>
              <Typography variant="body2">
                <strong>id:</strong> {createdKey.id} · <strong>name:</strong> {createdKey.name}
              </Typography>
              <Stack direction="row" spacing={1} alignItems="center">
                <TextField
                  value={createdKey.api_key}
                  fullWidth
                  inputProps={{ readOnly: true, style: { fontFamily: "monospace" } }}
                />
                <CopyToClipboard copy={createdKey.api_key} />
              </Stack>
            </Stack>
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCreatedKey(null)} variant="contained">
            I have copied the key
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
