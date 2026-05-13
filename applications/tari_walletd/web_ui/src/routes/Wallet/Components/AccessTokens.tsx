//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import {
  useAuthCreateApiKey,
  useAuthRevokeApiKey,
  useAuthRevokeToken,
  useGetAllApiKeys,
  useGetAllTokens,
} from "@api/hooks/useTokens";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { AccordionIconButton, CodeBlock, DataTableCell } from "@components/StyledComponents";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import VpnKeyIcon from "@mui/icons-material/VpnKey";
import {
  Alert,
  Box,
  Checkbox,
  Chip,
  Divider,
  Fade,
  FormControlLabel,
  FormGroup,
  List,
  ListItem,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import Button from "@mui/material/Button";
import Collapse from "@mui/material/Collapse";
import Dialog from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogContentText from "@mui/material/DialogContentText";
import DialogTitle from "@mui/material/DialogTitle";
import IconButton from "@mui/material/IconButton";
import type {
  AuthApiKeyInfo,
  AuthCreateApiKeyResponse,
  AuthSessionInfo,
  JrpcPermission,
  JrpcPermissions,
  RefreshTokenHash,
} from "@tari-project/ootle-ts-bindings";
import { jrpcPermissionToString } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import { IoCloseCircleOutline } from "react-icons/io5";

const API_KEY_PERMISSION_OPTIONS: { id: string; label: string; permission: JrpcPermission }[] = [
  { id: "account-info", label: "Account info", permission: "AccountInfo" },
  { id: "account-list", label: "Account list", permission: { AccountList: null } },
  { id: "substates-read", label: "Read substates", permission: "SubstatesRead" },
  { id: "templates-read", label: "Read templates", permission: "TemplatesRead" },
  { id: "key-list", label: "List keys", permission: "KeyList" },
  { id: "transaction-get", label: "Read transactions", permission: "TransactionGet" },
  { id: "transaction-send", label: "Send transactions", permission: { TransactionSend: null } },
  { id: "get-nft", label: "Read NFTs", permission: { GetNft: [null, null] } },
  { id: "nft-ownership", label: "NFT ownership proofs", permission: { NftGetOwnershipProof: null } },
  { id: "start-webrtc", label: "Start WebRTC", permission: "StartWebrtc" },
  { id: "address-book-read", label: "Address book read", permission: { AddressBook: "Read" } },
  { id: "address-book-create", label: "Address book create", permission: { AddressBook: "Create" } },
  { id: "address-book-update", label: "Address book update", permission: { AddressBook: "Update" } },
  { id: "address-book-delete", label: "Address book delete", permission: { AddressBook: "Delete" } },
  { id: "admin", label: "Admin", permission: "Admin" },
];
const API_KEY_NAME_MAX_LENGTH = 64;

function permissionLabel(permission: JrpcPermission) {
  if (typeof permission === "string") {
    return permission;
  }
  if ("AddressBook" in permission) {
    return `AddressBook(${permission.AddressBook})`;
  }
  if ("AccountList" in permission) {
    return `AccountList(${permission.AccountList ?? "any"})`;
  }
  if ("TransactionSend" in permission) {
    return `TransactionSend(${permission.TransactionSend ?? "any"})`;
  }
  if ("GetNft" in permission) {
    return `GetNft(${permission.GetNft[0] ?? "any"}, ${permission.GetNft[1] ?? "any"})`;
  }
  if ("NftGetOwnershipProof" in permission) {
    return `NftGetOwnershipProof(${permission.NftGetOwnershipProof ?? "any"})`;
  }
  return jrpcPermissionToString(permission);
}

function formatApiKeyDate(value: string | null, emptyLabel = "Never") {
  if (!value) {
    return emptyLabel;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
}

function apiKeyStatus(key: AuthApiKeyInfo) {
  if (key.revoked_at) {
    return { label: "Revoked", color: "default" as const };
  }
  if (key.expires_at) {
    const expiresAt = new Date(key.expires_at);
    if (!Number.isNaN(expiresAt.getTime()) && expiresAt.getTime() <= Date.now()) {
      return { label: "Expired", color: "warning" as const };
    }
  }
  return { label: "Active", color: "success" as const };
}

function AlertDialog({
  fn,
  title = "Revoke Token",
  message = "Would you like to revoke this token?",
  disabled = false,
}: {
  fn: () => void;
  title?: string;
  message?: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);

  const handleClickOpen = () => {
    setOpen(true);
  };

  const handleClose = () => {
    setOpen(false);
  };

  const handleRevokeClose = () => {
    fn();
    setOpen(false);
  };

  return (
    <div>
      <IconButton onClick={handleClickOpen} color="primary" disabled={disabled}>
        <IoCloseCircleOutline />
      </IconButton>
      <Dialog
        open={open}
        onClose={handleClose}
        aria-labelledby="alert-dialog-title"
        aria-describedby="alert-dialog-description"
      >
        <DialogTitle id="alert-dialog-title">{title}</DialogTitle>
        <DialogContent>
          <DialogContentText id="alert-dialog-description">{message}</DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button variant="outlined" onClick={handleClose}>
            No, Cancel
          </Button>
          <Button variant="contained" onClick={handleRevokeClose} autoFocus>
            Yes, Revoke
          </Button>
        </DialogActions>
      </Dialog>
    </div>
  );
}

export default function AccessTokens() {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const { data, isLoading, error, isError } = useGetAllTokens();
  const { mutate } = useAuthRevokeToken();

  const handleRevoke = async (id: RefreshTokenHash) => {
    mutate(id);
  };

  const emptyRows = page > 0 ? Math.max(0, (1 + page) * rowsPerPage - (data?.sessions.length || 0)) : 0;

  const handleChangePage = (_event: unknown, newPage: number) => {
    setPage(newPage);
  };

  const handleChangeRowsPerPage = (event: React.ChangeEvent<HTMLInputElement>) => {
    setRowsPerPage(parseInt(event.target.value, 10));
    setPage(0);
  };

  function RowData({
    id,
    permissions,
    formattedDate,
  }: {
    id: RefreshTokenHash;
    permissions: JrpcPermissions;
    formattedDate: string;
  }) {
    const [open, setOpen] = useState(false);

    return (
      <>
        <TableRow key={id}>
          <DataTableCell
            style={{
              borderBottom: "none",
            }}
          >
            {id}
          </DataTableCell>
          <DataTableCell
            style={{
              borderBottom: "none",
            }}
          >
            {formattedDate}
          </DataTableCell>
          <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }}>
            <AccordionIconButton
              aria-label="expand row"
              size="small"
              onClick={() => {
                setOpen(!open);
              }}
            >
              {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
            </AccordionIconButton>
          </DataTableCell>
          <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }}>
            <AlertDialog fn={() => handleRevoke(id)} />
          </DataTableCell>
        </TableRow>
        <TableRow>
          <DataTableCell
            style={{
              paddingBottom: 0,
              paddingTop: 0,
            }}
            colSpan={5}
          >
            <Collapse in={open} timeout="auto" unmountOnExit>
              <CodeBlock style={{ marginBottom: "10px" }}>
                Permissions:
                <List>
                  {permissions.map((item: JrpcPermission) => {
                    let permission = permissionLabel(item);
                    return <ListItem key={permission}>{permission}</ListItem>;
                  })}
                </List>
              </CodeBlock>
            </Collapse>
          </DataTableCell>
        </TableRow>
      </>
    );
  }

  return (
    <>
      <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
        <Fade in={!isLoading && !isError}>
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>ID</TableCell>
                  <TableCell>Expiry Date</TableCell>
                  <TableCell align="center">Permissions</TableCell>
                  <TableCell width="100" align="center">
                    Revoke
                  </TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {data?.sessions
                  ?.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage)
                  .map(({ id, permissions, exp }: AuthSessionInfo) => {
                    const date = new Date(Number(exp) * 1000);
                    const formattedDate = `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
                    return <RowData key={id} id={id} permissions={permissions} formattedDate={formattedDate} />;
                  })}

                {emptyRows > 0 && (
                  <TableRow style={{ height: 57 * emptyRows }}>
                    <TableCell colSpan={4} />
                  </TableRow>
                )}
              </TableBody>
            </Table>
            {data?.sessions && (
              <TablePagination
                rowsPerPageOptions={[10, 25, 50]}
                component="div"
                count={data.sessions.length}
                rowsPerPage={rowsPerPage}
                page={page}
                onPageChange={handleChangePage}
                onRowsPerPageChange={handleChangeRowsPerPage}
              />
            )}
          </TableContainer>
        </Fade>
      </FetchStatusCheck>
      <Divider sx={{ my: 4 }} />
      <ApiKeys />
    </>
  );
}

function ApiKeys() {
  const [name, setName] = useState("");
  const [selectedPermissionIds, setSelectedPermissionIds] = useState<string[]>([
    "account-info",
    "transaction-get",
  ]);
  const [confirmAdmin, setConfirmAdmin] = useState(false);
  const [createdKey, setCreatedKey] = useState<AuthCreateApiKeyResponse | null>(null);
  const { data, isLoading, isError, error } = useGetAllApiKeys();
  const createApiKey = useAuthCreateApiKey();
  const revokeApiKey = useAuthRevokeApiKey();

  const selectedPermissions = API_KEY_PERMISSION_OPTIONS.filter((option) =>
    selectedPermissionIds.includes(option.id),
  ).map((option) => option.permission);
  const includesAdmin = selectedPermissions.some((permission) => permission === "Admin");
  const canCreate = name.trim().length > 0 && selectedPermissions.length > 0 && (!includesAdmin || confirmAdmin);

  const handleTogglePermission = (id: string) => {
    setSelectedPermissionIds((current) =>
      current.includes(id) ? current.filter((permissionId) => permissionId !== id) : [...current, id],
    );
  };

  const handleCreate = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!canCreate) {
      return;
    }
    const response = await createApiKey.mutateAsync({
      name,
      permissions: selectedPermissions,
      confirm_admin: confirmAdmin,
    });
    setCreatedKey(response);
    setName("");
    setConfirmAdmin(false);
  };

  const copyCreatedKey = async () => {
    if (createdKey) {
      await navigator.clipboard.writeText(createdKey.api_key);
    }
  };

  return (
    <Stack spacing={3}>
      <Box component="form" onSubmit={handleCreate}>
        <Stack spacing={2}>
          <Stack direction="row" spacing={1.5} alignItems="center">
            <VpnKeyIcon color="primary" />
            <Typography variant="h6">Agent API Keys</Typography>
          </Stack>
          <TextField
            label="Name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="automation-runner"
            fullWidth
            slotProps={{ htmlInput: { maxLength: API_KEY_NAME_MAX_LENGTH } }}
          />
          <FormGroup
            sx={{
              display: "grid",
              gridTemplateColumns: { xs: "1fr", md: "repeat(2, minmax(0, 1fr))" },
              gap: 0.5,
            }}
          >
            {API_KEY_PERMISSION_OPTIONS.map((option) => (
              <FormControlLabel
                key={option.id}
                control={
                  <Checkbox
                    checked={selectedPermissionIds.includes(option.id)}
                    onChange={() => handleTogglePermission(option.id)}
                  />
                }
                label={option.label}
              />
            ))}
          </FormGroup>
          {includesAdmin && (
            <Alert severity="warning">
              Admin API keys can create and revoke other API keys and can access every wallet RPC.
            </Alert>
          )}
          <FormControlLabel
            control={
              <Checkbox
                checked={confirmAdmin}
                disabled={!includesAdmin}
                onChange={(event) => setConfirmAdmin(event.target.checked)}
              />
            }
            label="Confirm Admin grant"
          />
          <Box>
            <Button
              type="submit"
              variant="contained"
              startIcon={<VpnKeyIcon />}
              disabled={!canCreate || createApiKey.isPending}
            >
              Create API Key
            </Button>
          </Box>
        </Stack>
      </Box>

      <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow>
                <TableCell>ID</TableCell>
                <TableCell>Name</TableCell>
                <TableCell>Status</TableCell>
                <TableCell>Created</TableCell>
                <TableCell>Last Used</TableCell>
                <TableCell>Expires</TableCell>
                <TableCell>Permissions</TableCell>
                <TableCell width="100" align="center">
                  Revoke
                </TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {data?.api_keys.length === 0 && (
                <TableRow>
                  <TableCell colSpan={7} align="center">
                    No agent API keys
                  </TableCell>
                </TableRow>
              )}
              {data?.api_keys.map((key: AuthApiKeyInfo) => {
                const status = apiKeyStatus(key);
                return (
                  <TableRow key={key.id}>
                    <DataTableCell>{key.id}</DataTableCell>
                    <TableCell>{key.name}</TableCell>
                    <TableCell>
                      <Chip size="small" label={status.label} color={status.color} />
                    </TableCell>
                    <TableCell>{formatApiKeyDate(key.created_at)}</TableCell>
                    <TableCell>{formatApiKeyDate(key.last_used_at)}</TableCell>
                    <TableCell>{formatApiKeyDate(key.expires_at, "No expiry")}</TableCell>
                    <TableCell>
                      <Stack direction="row" spacing={1} useFlexGap flexWrap="wrap">
                        {key.permissions.map((permission) => (
                          <Chip key={permissionLabel(permission)} label={permissionLabel(permission)} size="small" />
                        ))}
                      </Stack>
                    </TableCell>
                    <TableCell align="center">
                      <Tooltip title={key.revoked_at ? "Already revoked" : "Revoke API key"}>
                        <span>
                          <IconButton
                            color="primary"
                            disabled={Boolean(key.revoked_at) || revokeApiKey.isPending}
                            onClick={() => revokeApiKey.mutate(key.id)}
                          >
                            <DeleteOutlineIcon />
                          </IconButton>
                        </span>
                      </Tooltip>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </TableContainer>
      </FetchStatusCheck>

      <Dialog open={createdKey !== null} onClose={() => setCreatedKey(null)} maxWidth="md" fullWidth>
        <DialogTitle>API Key Created</DialogTitle>
        <DialogContent>
          <DialogContentText sx={{ mb: 2 }}>
            The raw API key is shown once. Store it before closing this dialog.
          </DialogContentText>
          <CodeBlock>{createdKey?.api_key}</CodeBlock>
        </DialogContent>
        <DialogActions>
          <Button variant="outlined" startIcon={<ContentCopyIcon />} onClick={copyCreatedKey}>
            Copy
          </Button>
          <Button variant="contained" onClick={() => setCreatedKey(null)}>
            Done
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
