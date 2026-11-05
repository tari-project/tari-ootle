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
import { decodeOotleAddressOrNull } from "@tari-project/ootle-ts-bindings";
import { shortenString } from "@utils/helpers";
import { useMemo, useState } from "react";
import { MdCheckCircle, MdPerson, MdPersonAdd } from "react-icons/md";

function formatDestinationTag(destinationTag: string | Uint8Array | null | undefined): string | null {
  if (!destinationTag) return null;
  if (typeof destinationTag === "string") return destinationTag.length > 0 ? destinationTag : null;
  if (destinationTag.length === 0) return null;
  // Non-UTF-8 bytes: show as hex so the user can still copy/identify the value.
  return Array.from(destinationTag)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

// Identity key for "is this the same account?" comparisons. We drop the optional destination tag so an incoming
// SenderAddress with a destination tag still matches the wallet's own account address (which has no tag) or an
// address-book entry stored without one.
function identityKey(address: string): string | null {
  const decoded = decodeOotleAddressOrNull(address);
  if (!decoded) return null;
  return `${decoded.accountPublicKey}:${decoded.viewOnlyKey}`;
}

export function SenderAddress({ address }: { address: string }) {
  const { data: addressBook } = useAddressBookList();
  const { data: accountsData } = useAccountsList(0, 100);
  const addMutation = useAddressBookAdd();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [name, setName] = useState("");
  const [note, setNote] = useState("");
  const [formError, setFormError] = useState<string | null>(null);

  // The sender may be one of the wallet's own accounts (e.g. a self-transfer or change) — there's nothing to
  // "add to contacts" in that case. We compare on the (account_key, view_key) identity so a SenderAddress that
  // carries a destination tag still matches the bare account or address-book entry.
  const senderIdentity = useMemo(() => identityKey(address), [address]);
  const ownAccount = useMemo(
    () => accountsData?.accounts?.find((a) => senderIdentity !== null && identityKey(a.address) === senderIdentity),
    [accountsData, senderIdentity],
  );
  const existing = useMemo(
    () =>
      addressBook?.entries?.find((entry) => senderIdentity !== null && identityKey(entry.address) === senderIdentity),
    [addressBook, senderIdentity],
  );
  const destinationTagText = useMemo(
    () => formatDestinationTag(decodeOotleAddressOrNull(address)?.destinationTag ?? null),
    [address],
  );

  const handleClose = () => {
    setDialogOpen(false);
    setName("");
    setNote("");
    setFormError(null);
  };

  const handleSave = async () => {
    if (!name.trim()) {
      setFormError("Name is required");
      return;
    }
    try {
      await addMutation.mutateAsync({ name: name.trim(), address, note: note.trim() || null });
      handleClose();
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
          <Button onClick={handleClose}>Cancel</Button>
          <Button onClick={handleSave} variant="contained" disabled={addMutation.isPending}>
            Add
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}

export default SenderAddress;
