// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useAccountsList } from "@api/hooks/useAccounts";
import { useAddressBookAdd, useAddressBookList } from "@api/hooks/useAddressBook";
import CopyToClipboard from "@components/CopyToClipboard";
import {
  Alert,
  Button,
  Chip,
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
import { useEffect, useMemo, useRef, useState } from "react";
import { MdCheckCircle, MdPerson, MdPersonAdd } from "react-icons/md";
import { formatAddressBookAddError, identityKey, parseSenderAddress } from "./senderAddressHelpers";

export function SenderAddress({ address }: { address: string }) {
  const { data: addressBook } = useAddressBookList();
  const { data: accountsData } = useAccountsList(0, 100);
  const addMutation = useAddressBookAdd();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [name, setName] = useState("");
  const [note, setNote] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const sessionRef = useRef(0);

  // The sender may be one of the wallet's own accounts (e.g. a self-transfer or change) — there's nothing to
  // "add to contacts" in that case. We compare on the (account_key, view_key) identity so a SenderAddress that
  // carries a destination tag still matches the bare account or address-book entry.
  const { identityKey: senderIdentity, destinationTagText } = useMemo(() => parseSenderAddress(address), [address]);
  const ownAccount = useMemo(
    () =>
      senderIdentity === null
        ? undefined
        : accountsData?.accounts?.find((a) => identityKey(a.address) === senderIdentity),
    [accountsData, senderIdentity],
  );
  const existing = useMemo(
    () =>
      senderIdentity === null
        ? undefined
        : addressBook?.entries?.find((entry) => identityKey(entry.address) === senderIdentity),
    [addressBook, senderIdentity],
  );

  const resetDialog = () => {
    setDialogOpen(false);
    setName("");
    setNote("");
    setFormError(null);
  };

  useEffect(() => {
    sessionRef.current += 1;
    resetDialog();
    return () => {
      sessionRef.current += 1;
    };
  }, [address]);

  const handleClose = () => {
    if (addMutation.isPending) {
      return;
    }
    resetDialog();
  };

  const handleSave = async () => {
    if (addMutation.isPending) {
      return;
    }
    if (!name.trim()) {
      setFormError("Name is required");
      return;
    }
    const requestSession = sessionRef.current;
    try {
      await addMutation.mutateAsync({ name: name.trim(), address, note: note.trim() || null });
      if (sessionRef.current === requestSession) {
        resetDialog();
      }
    } catch (e: unknown) {
      if (sessionRef.current === requestSession) {
        setFormError(formatAddressBookAddError(e));
      }
    }
  };

  return (
    <Stack direction="row" spacing={1} alignItems="center">
      <Tooltip title={`Sender: ${address}`}>
        <span>{shortenString(address)}</span>
      </Tooltip>
      <CopyToClipboard copy={address} />
      {destinationTagText && (
        <Tooltip title={`Destination tag: ${destinationTagText}`}>
          <Chip size="small" color="info" variant="outlined" label={`ref: ${shortenString(destinationTagText)}`} />
        </Tooltip>
      )}
      {ownAccount ? (
        <Tooltip title="This is one of your own accounts">
          <Typography variant="body2" color="text.secondary" sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <MdPerson /> {ownAccount.account.name ?? "Your account"}
          </Typography>
        </Tooltip>
      ) : existing ? (
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

      <Dialog open={dialogOpen} onClose={handleClose} maxWidth="sm" fullWidth>
        <DialogTitle>Add to Contacts</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1 }}>
            {formError && <Alert severity="error">{formError}</Alert>}
            <TextField
              label="Name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={addMutation.isPending}
              required
              fullWidth
              autoFocus
            />
            <TextField label="Address" value={address} fullWidth disabled />
            <TextField
              label="Note (optional)"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              disabled={addMutation.isPending}
              fullWidth
              multiline
              rows={2}
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={handleClose} disabled={addMutation.isPending}>
            Cancel
          </Button>
          <Button onClick={handleSave} variant="contained" disabled={addMutation.isPending}>
            Add
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}

export default SenderAddress;
