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
import {
  Alert,
  Button,
  Checkbox,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
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
import type { AuthCreateApiKeyResponse, IssuedApiKey } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";

/// Render a unix-second timestamp as a locale string. `null` -> "never".
function fmtTs(ts: bigint | null | undefined): string {
  if (ts == null) {
    return "never";
  }
  return new Date(Number(ts) * 1000).toLocaleString();
}

export default function ApiKeys() {
  const { data, isLoading, error, isError } = useListApiKeys();
  const { mutate: revoke } = useRevokeApiKey();

  // Stash the create-result here so the dialog can show the raw key
  // exactly once. Set back to null on dialog close — the key string
  // is never persisted in component state across re-renders beyond
  // that dialog's lifetime.
  const [createdKey, setCreatedKey] = useState<AuthCreateApiKeyResponse | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createName, setCreateName] = useState("");
  const [createPermsRaw, setCreatePermsRaw] = useState("");
  const [confirmAdmin, setConfirmAdmin] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const { mutate: createKey, isPending: isCreating } = useCreateApiKey((response) => {
    setCreatedKey(response);
    setCreateOpen(false);
    setCreateName("");
    setCreatePermsRaw("");
    setConfirmAdmin(false);
    setCreateError(null);
  });

  const handleCreate = () => {
    setCreateError(null);
    const permissions = createPermsRaw
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
    if (permissions.length === 0) {
      setCreateError("At least one permission must be granted; an unusable key is refused by the daemon.");
      return;
    }
    if (permissions.includes("Admin") && !confirmAdmin) {
      setCreateError("Granting the Admin permission requires explicit confirmation. Tick the box below to proceed.");
      return;
    }
    createKey(
      { name: createName, permissions, confirm_admin: confirmAdmin },
      {
        onError: (err: unknown) => {
          setCreateError(err instanceof Error ? err.message : "Create failed");
        },
      },
    );
  };

  return (
    <Stack spacing={2}>
      <Alert severity="info">
        API keys are long-lived credentials used by AI agents and other automated clients. They authenticate without
        WebAuthn and are bounded to the permission scopes you choose at creation time. Revocation is immediate. The raw
        key is shown exactly once — the daemon only stores its SHA-256 hash.
      </Alert>

      <Stack direction="row" justifyContent="flex-end">
        <Button variant="contained" onClick={() => setCreateOpen(true)}>
          Create API Key
        </Button>
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
                {(data?.keys || []).map((k: IssuedApiKey) => {
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
                        <Chip
                          label={revoked ? "REVOKED" : "active"}
                          color={revoked ? "default" : "success"}
                          size="small"
                        />
                      </TableCell>
                      <TableCell align="right">
                        <Button
                          variant="outlined"
                          size="small"
                          disabled={revoked}
                          onClick={() => revoke({ id: k.id })}
                        >
                          Revoke
                        </Button>
                      </TableCell>
                    </TableRow>
                  );
                })}
                {(data?.keys || []).length === 0 && (
                  <TableRow>
                    <TableCell colSpan={7} align="center">
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
      <Dialog open={createOpen} onClose={() => setCreateOpen(false)} maxWidth="sm" fullWidth>
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
            <TextField
              label="Permissions (comma-separated)"
              value={createPermsRaw}
              onChange={(e) => setCreatePermsRaw(e.target.value)}
              helperText='e.g. "AccountInfo, TransactionGet". Same form JrpcPermissions::from_str accepts.'
              fullWidth
            />
            <FormControlLabel
              control={
                <Checkbox checked={confirmAdmin} onChange={(e) => setConfirmAdmin(e.target.checked)} />
              }
              label="I understand granting Admin gives full access (only required if Admin is in the list above)"
            />
            {createError && <Alert severity="error">{createError}</Alert>}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCreateOpen(false)}>Cancel</Button>
          <Button onClick={handleCreate} variant="contained" disabled={isCreating || createName.trim().length === 0}>
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
            manager. The daemon persists only a SHA-256 hash and cannot surface it again.
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
