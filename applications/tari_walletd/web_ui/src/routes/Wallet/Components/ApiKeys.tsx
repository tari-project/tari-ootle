//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useCreateApiKey, useListApiKeys, useRevokeApiKey } from "@api/hooks/useApiKeys";
import CopyToClipboard from "@components/CopyToClipboard";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { StyledPaper } from "@components/StyledComponents";
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
  FormControlLabel,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Typography,
} from "@mui/material";
import type { ApiKeyInfo, AuthCreateApiKeyResponse } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";

/// Permissions offered as selectable checkboxes — the 10 unparameterised
/// forms that JrpcPermissions::from_str accepts verbatim.
/// Admin is in a separate section with an explicit confirmation checkbox.
const PERMISSION_OPTIONS: Array<{ value: string; label: string; description: string }> = [
  { value: "AccountInfo",        label: "AccountInfo",        description: "Read account metadata (address, public keys, name)." },
  { value: "AccountList",        label: "AccountList",        description: "Enumerate accounts known to the wallet." },
  { value: "KeyList",            label: "KeyList",            description: "Enumerate keys held by the key manager." },
  { value: "TransactionGet",     label: "TransactionGet",     description: "Read transaction history and detail." },
  { value: "TransactionSend",    label: "TransactionSend",    description: "Submit transactions from any account." },
  { value: "SubstatesRead",      label: "SubstatesRead",      description: "Read on-chain substate data." },
  { value: "TemplatesRead",      label: "TemplatesRead",      description: "Read deployed contract templates." },
  { value: "NftGetOwnershipProof",label: "NftGetOwnershipProof",description: "Produce ownership proofs for owned NFTs." },
  { value: "GetNft",             label: "GetNft",             description: "Read NFT data the wallet holds." },
  { value: "StartWebrtc",        label: "StartWebrtc",        description: "Initiate the WebRTC signalling flow." },
];

const ADMIN_PERMISSION_VALUE = "Admin";

function fmtTs(ts: bigint | null | undefined): string {
  if (ts == null) return "never";
  return new Date(Number(ts) * 1000).toLocaleString();
}

export default function ApiKeys() {
  const { data, isLoading, error, isError } = useListApiKeys();
  const { mutate: revoke } = useRevokeApiKey();

  const [createdKey, setCreatedKey] = useState<AuthCreateApiKeyResponse | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createName, setCreateName] = useState("");
  const [selectedPerms, setSelectedPerms] = useState<Set<string>>(() => new Set());
  const [grantAdmin, setGrantAdmin] = useState(false);
  const [confirmAdmin, setConfirmAdmin] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const resetCreateForm = () => {
    setCreateName("");
    setSelectedPerms(new Set());
    setGrantAdmin(false);
    setConfirmAdmin(false);
    setCreateError(null);
  };

  const togglePermission = (value: string) => {
    setSelectedPerms((current) => {
      const next = new Set(current);
      if (next.has(value)) next.delete(value);
      else next.add(value);
      return next;
    });
  };

  const { mutate: createKey, isPending: isCreating } = useCreateApiKey((response) => {
    setCreatedKey(response);
    setCreateOpen(false);
    resetCreateForm();
  });

  const handleCreate = () => {
    setCreateError(null);
    const permissions = PERMISSION_OPTIONS.filter((opt) => selectedPerms.has(opt.value)).map((opt) => opt.value);
    if (grantAdmin) permissions.push(ADMIN_PERMISSION_VALUE);
    if (permissions.length === 0) {
      setCreateError("Select at least one permission.");
      return;
    }
    if (grantAdmin && !confirmAdmin) {
      setCreateError("Tick the confirmation box to grant Admin.");
      return;
    }
    createKey(
      { name: createName, permissions, confirm_admin: grantAdmin && confirmAdmin },
      { onError: (err: unknown) => setCreateError(err instanceof Error ? err.message : "Create failed") },
    );
  };

  return (
    <Stack spacing={2}>
      <Alert severity="info">
        API keys are long-lived credentials for AI agents and automated clients. They authenticate without WebAuthn
        and are bounded to the scopes chosen at creation. Revocation is immediate. The raw key is shown exactly
        once — the daemon stores only a SHA-256 hash.
      </Alert>

      <Stack direction="row" justifyContent="flex-end">
        <Button variant="contained" onClick={() => setCreateOpen(true)}>Create API Key</Button>
      </Stack>

      <FetchStatusCheck errorMessage={error?.message || "Error fetching API keys"} isError={isError} isLoading={isLoading}>
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
                  <TableCell>Status</TableCell>
                  <TableCell align="right">Actions</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {(data?.keys || []).map((k: ApiKeyInfo) => {
                  const revoked = k.revoked_at != null;
                  return (
                    <TableRow key={k.id}>
                      <TableCell>{k.id}</TableCell>
                      <TableCell>{k.name}</TableCell>
                      <TableCell>
                        <Stack direction="row" spacing={0.5} flexWrap="wrap">
                          {k.permissions.map((p, idx) => (
                            <Chip key={idx} label={JSON.stringify(p)} size="small" />
                          ))}
                        </Stack>
                      </TableCell>
                      <TableCell>{fmtTs(k.created_at)}</TableCell>
                      <TableCell>{fmtTs(k.last_used_at)}</TableCell>
                      <TableCell>
                        <Chip label={revoked ? "REVOKED" : "active"} color={revoked ? "default" : "success"} size="small" />
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
                  <TableRow><TableCell colSpan={7} align="center">No API keys issued.</TableCell></TableRow>
                )}
              </TableBody>
            </Table>
          </TableContainer>
        </StyledPaper>
      </FetchStatusCheck>

      {/* Create dialog */}
      <Dialog open={createOpen} onClose={() => { setCreateOpen(false); resetCreateForm(); }} maxWidth="sm" fullWidth>
        <DialogTitle>Create API Key</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1 }}>
            <TextField
              label="Name"
              value={createName}
              onChange={(e) => setCreateName(e.target.value)}
              helperText="A friendly label for this key."
              fullWidth
            />
            <Box>
              <Typography variant="subtitle2" sx={{ mb: 0.5 }}>Permissions</Typography>
              <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
                Pick the smallest set the agent needs.
              </Typography>
              <Stack>
                {PERMISSION_OPTIONS.map((opt) => (
                  <FormControlLabel
                    key={opt.value}
                    sx={{ alignItems: "flex-start", mr: 0, my: 0.25 }}
                    control={
                      <Checkbox
                        size="small"
                        sx={{ pt: 0.5 }}
                        checked={selectedPerms.has(opt.value)}
                        onChange={() => togglePermission(opt.value)}
                      />
                    }
                    label={
                      <Box>
                        <Typography variant="body2" component="span">{opt.label}</Typography>
                        <Typography variant="caption" color="text.secondary" sx={{ display: "block" }}>
                          {opt.description}
                        </Typography>
                      </Box>
                    }
                  />
                ))}
              </Stack>
              <Divider sx={{ my: 1.5 }} />
              <FormControlLabel
                sx={{ alignItems: "flex-start", mr: 0 }}
                control={
                  <Checkbox
                    size="small"
                    color="warning"
                    sx={{ pt: 0.5 }}
                    checked={grantAdmin}
                    onChange={(e) => { setGrantAdmin(e.target.checked); if (!e.target.checked) setConfirmAdmin(false); }}
                  />
                }
                label={
                  <Box>
                    <Typography variant="body2" component="span" color="warning.main">Admin</Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block" }}>
                      Full daemon access. Avoid granting to long-lived agent keys unless absolutely required.
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
            {createError && <Alert severity="error">{createError}</Alert>}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => { setCreateOpen(false); resetCreateForm(); }}>Cancel</Button>
          <Button
            onClick={handleCreate}
            variant="contained"
            disabled={isCreating || createName.trim().length === 0 || (selectedPerms.size === 0 && !grantAdmin)}
          >
            Create
          </Button>
        </DialogActions>
      </Dialog>

      {/* Show-once key dialog */}
      <Dialog open={createdKey != null} onClose={() => setCreatedKey(null)} maxWidth="sm" fullWidth>
        <DialogTitle>API Key Created</DialogTitle>
        <DialogContent>
          <DialogContentText sx={{ mb: 2 }}>
            <strong>This is the only time the raw key will be shown.</strong> Copy it now and store it in a secrets
            manager. The daemon persists only a SHA-256 hash and cannot surface it again.
          </DialogContentText>
          {createdKey && (
            <Stack spacing={1}>
              <Typography variant="body2">
                <strong>id:</strong> {createdKey.id} · <strong>name:</strong> {createdKey.name}
              </Typography>
              <Stack direction="row" spacing={1} alignItems="center">
                <TextField
                  value={createdKey.key}
                  fullWidth
                  inputProps={{ readOnly: true, style: { fontFamily: "monospace" } }}
                />
                <CopyToClipboard copy={createdKey.key} />
              </Stack>
            </Stack>
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCreatedKey(null)} variant="contained">I have copied the key</Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
