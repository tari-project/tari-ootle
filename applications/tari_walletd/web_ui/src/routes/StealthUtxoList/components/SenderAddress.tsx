// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useAddressBookAdd, useAddressBookList } from "@api/hooks/useAddressBook";
import CopyToClipboard from "@components/CopyToClipboard";
import {
  Alert,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { shortenString } from "@utils/helpers";
import { useState } from "react";
import { MdCheckCircle, MdPersonAdd } from "react-icons/md";

export function SenderAddress({ address }: { address: string }) {
  const { data } = useAddressBookList();
  const addMutation = useAddressBookAdd();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [name, setName] = useState("");
  const [note, setNote] = useState("");
  const [formError, setFormError] = useState<string | null>(null);

  const existing = data?.entries?.find((entry) => entry.address === address);

  const handleSave = async () => {
    if (!name.trim()) {
      setFormError("Name is required");
      return;
    }
    try {
      await addMutation.mutateAsync({ name: name.trim(), address, note: note.trim() || null });
      setDialogOpen(false);
      setName("");
      setNote("");
    } catch (e: any) {
      const msg = e?.cause?.message || e?.message || "Failed to add contact";
      setFormError(msg.includes("DuplicateName") ? "An entry with this name already exists" : msg);
    }
  };

  return (
    <Stack direction="row" spacing={1} alignItems="center">
      <Tooltip title={`Sender: ${address}`}>
        <span>{shortenString(address)}</span>
      </Tooltip>
      <CopyToClipboard copy={address} />
      {existing ? (
        <Tooltip title={`Saved as "${existing.name}"`}>
          <Typography variant="body2" color="success.main" sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <MdCheckCircle /> {existing.name}
          </Typography>
        </Tooltip>
      ) : (
        <Button
          size="small"
          variant="outlined"
          startIcon={<MdPersonAdd />}
          onClick={() => {
            setFormError(null);
            setDialogOpen(true);
          }}
        >
          Add to contacts
        </Button>
      )}

      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} maxWidth="sm" fullWidth>
        <DialogTitle>Add to Contacts</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1 }}>
            {formError && <Alert severity="error">{formError}</Alert>}
            <TextField
              label="Name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              fullWidth
              autoFocus
            />
            <TextField label="Address" value={address} fullWidth disabled />
            <TextField
              label="Note (optional)"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              fullWidth
              multiline
              rows={2}
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)}>Cancel</Button>
          <Button onClick={handleSave} variant="contained" disabled={addMutation.isPending}>
            Add
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}

export default SenderAddress;
