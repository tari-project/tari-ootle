// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import PageHeading from "@components/PageHeading";
import { StyledPaper } from "@components/StyledComponents";
import {
  useAddressBookAdd,
  useAddressBookDelete,
  useAddressBookList,
  useAddressBookUpdate,
} from "@api/hooks/useAddressBook";
import type { AddressBookEntry } from "@tari-project/ootle-ts-bindings";
import { validateOotleAddress } from "@tari-project/ootle-ts-bindings";
import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import Grid from "@mui/material/Grid";
import { DataGrid, GridColDef, GridRenderCellParams } from "@mui/x-data-grid";
import { useState } from "react";
import { MdAdd, MdDelete, MdEdit } from "react-icons/md";

interface EntryFormState {
  name: string;
  address: string;
  memo: string;
}

const EMPTY_FORM: EntryFormState = { name: "", address: "", memo: "" };

export default function AddressBookPage() {
  const { data, isLoading } = useAddressBookList();
  const addMutation = useAddressBookAdd();
  const updateMutation = useAddressBookUpdate();
  const deleteMutation = useAddressBookDelete();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingEntry, setEditingEntry] = useState<AddressBookEntry | null>(null);
  const [form, setForm] = useState<EntryFormState>(EMPTY_FORM);
  const [formError, setFormError] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  const entries = data?.entries ?? [];

  const openAddDialog = () => {
    setEditingEntry(null);
    setForm(EMPTY_FORM);
    setFormError(null);
    setDialogOpen(true);
  };

  const openEditDialog = (entry: AddressBookEntry) => {
    setEditingEntry(entry);
    setForm({ name: entry.name, address: entry.address, memo: entry.memo ?? "" });
    setFormError(null);
    setDialogOpen(true);
  };

  const handleSave = async () => {
    if (!form.name.trim()) {
      setFormError("Name is required");
      return;
    }
    if (!form.address.trim()) {
      setFormError("Address is required");
      return;
    }
    if (!validateOotleAddress(form.address.trim())) {
      setFormError("Invalid Ootle address");
      return;
    }

    try {
      if (editingEntry) {
        await updateMutation.mutateAsync({
          name: editingEntry.name,
          new_name: form.name !== editingEntry.name ? form.name.trim() : undefined,
          address: form.address !== editingEntry.address ? form.address.trim() : undefined,
          memo: form.memo.trim() || undefined,
        });
      } else {
        await addMutation.mutateAsync({
          name: form.name.trim(),
          address: form.address.trim(),
          memo: form.memo.trim() || undefined,
        });
      }
      setDialogOpen(false);
    } catch (e: any) {
      const msg = e?.cause?.message || e?.message || "Failed to save entry";
      if (msg.toLowerCase().includes("unique") || msg.toLowerCase().includes("duplicate")) {
        setFormError("An entry with this name already exists");
      } else {
        setFormError(msg);
      }
    }
  };

  const handleDelete = async (name: string) => {
    try {
      await deleteMutation.mutateAsync({ name });
      setDeleteConfirm(null);
    } catch {
      // Error handling via mutation state
    }
  };

  const columns: GridColDef[] = [
    { field: "name", headerName: "Name", flex: 1, minWidth: 120 },
    { field: "address", headerName: "Address", flex: 2, minWidth: 200 },
    { field: "memo", headerName: "Memo", flex: 1, minWidth: 120 },
    {
      field: "actions",
      headerName: "",
      width: 100,
      sortable: false,
      renderCell: (params: GridRenderCellParams<AddressBookEntry>) => (
        <Stack direction="row" spacing={0.5}>
          <IconButton size="small" onClick={() => openEditDialog(params.row)}>
            <MdEdit />
          </IconButton>
          <IconButton size="small" onClick={() => setDeleteConfirm(params.row.name)} color="error">
            <MdDelete />
          </IconButton>
        </Stack>
      ),
    },
  ];

  return (
    <>
      <Grid size={12}>
        <PageHeading>Address Book</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 2 }}>
            <Typography variant="h6">Saved Addresses</Typography>
            <Button variant="contained" startIcon={<MdAdd />} onClick={openAddDialog}>
              Add Entry
            </Button>
          </Box>

          <DataGrid
            rows={entries}
            columns={columns}
            loading={isLoading}
            autoHeight
            disableRowSelectionOnClick
            pageSizeOptions={[10, 25]}
            initialState={{ pagination: { paginationModel: { pageSize: 10 } } }}
            sx={{ border: 0 }}
          />
        </StyledPaper>
      </Grid>

      {/* Add/Edit Dialog */}
      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} maxWidth="sm" fullWidth>
        <DialogTitle>{editingEntry ? "Edit Entry" : "Add Entry"}</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1 }}>
            {formError && <Alert severity="error">{formError}</Alert>}
            <TextField
              label="Name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              required
              fullWidth
            />
            <TextField
              label="Address"
              value={form.address}
              onChange={(e) => setForm({ ...form, address: e.target.value })}
              required
              fullWidth
              placeholder="otl_loc_..."
            />
            <TextField
              label="Memo (optional)"
              value={form.memo}
              onChange={(e) => setForm({ ...form, memo: e.target.value })}
              fullWidth
              multiline
              rows={2}
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)}>Cancel</Button>
          <Button
            onClick={handleSave}
            variant="contained"
            disabled={addMutation.isPending || updateMutation.isPending}
          >
            {editingEntry ? "Update" : "Add"}
          </Button>
        </DialogActions>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <Dialog open={deleteConfirm !== null} onClose={() => setDeleteConfirm(null)}>
        <DialogTitle>Delete Entry</DialogTitle>
        <DialogContent>
          <Typography>
            Are you sure you want to delete &quot;{deleteConfirm}&quot; from your address book?
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDeleteConfirm(null)}>Cancel</Button>
          <Button
            onClick={() => deleteConfirm && handleDelete(deleteConfirm)}
            color="error"
            variant="contained"
            disabled={deleteMutation.isPending}
          >
            Delete
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
