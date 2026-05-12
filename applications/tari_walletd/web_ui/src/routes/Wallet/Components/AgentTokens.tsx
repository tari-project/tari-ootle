//  Copyright 2026 The Tari Project
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

import { useGetAgentTokens, useRevokeAgentToken } from "@api/hooks/useAgentTokens";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { BoxHeading2, DataTableCell } from "@components/StyledComponents";
import AddIcon from "@mui/icons-material/Add";
import {
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Fade,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
  Typography,
} from "@mui/material";
import { jrpcPermissionToString, type JrpcPermission } from "@tari-project/ootle-ts-bindings";
import { useMemo, useState } from "react";
import CreateAgentTokenDialog from "./CreateAgentTokenDialog";

function formatTimestamp(timestamp: number | null) {
  if (!timestamp) return "Never";
  const date = new Date(Number(timestamp) * 1000);
  return `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
}

function RevokeAgentTokenDialog({ onConfirm }: { onConfirm: () => void }) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button variant="outlined" color="error" size="small" onClick={() => setOpen(true)}>
        Revoke
      </Button>
      <Dialog open={open} onClose={() => setOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle>Revoke agent token</DialogTitle>
        <DialogContent>
          <DialogContentText>Would you like to revoke this agent token?</DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button variant="outlined" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button
            variant="contained"
            color="error"
            onClick={() => {
              onConfirm();
              setOpen(false);
            }}
          >
            Revoke
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}

function PermissionPills({ permissions }: { permissions: JrpcPermission[] }) {
  return (
    <Stack direction="row" spacing={0.75} useFlexGap flexWrap="wrap">
      {permissions.map((permission) => {
        const label = jrpcPermissionToString(permission);
        return <Chip key={label} size="small" label={label} variant="outlined" />;
      })}
    </Stack>
  );
}

export default function AgentTokens() {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [dialogOpen, setDialogOpen] = useState(false);
  const { data, isLoading, error, isError } = useGetAgentTokens();
  const { mutate: revokeToken } = useRevokeAgentToken();

  const keys = data?.keys ?? [];
  const pagedKeys = useMemo(
    () => keys.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage),
    [keys, page, rowsPerPage],
  );
  const emptyRows = page > 0 ? Math.max(0, (1 + page) * rowsPerPage - keys.length) : 0;

  return (
    <>
      <BoxHeading2
        sx={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          gap: 2,
          flexWrap: "wrap",
        }}
      >
        <Typography variant="body1" color="text.secondary">
          Manage long-lived agent tokens for scoped wallet access.
        </Typography>
        <Button variant="outlined" startIcon={<AddIcon />} onClick={() => setDialogOpen(true)}>
          Create agent token
        </Button>
      </BoxHeading2>

      <CreateAgentTokenDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onCreated={() => setDialogOpen(false)}
      />

      <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
        <Fade in={!isLoading && !isError}>
          <Box>
            {keys.length === 0 ? (
              <Box
                sx={{
                  py: 6,
                  textAlign: "center",
                  color: "text.secondary",
                }}
              >
                No agent tokens yet. Create one to allow AI agents to access this wallet.
              </Box>
            ) : (
              <TableContainer>
                <Table>
                  <TableHead>
                    <TableRow>
                      <TableCell>Name</TableCell>
                      <TableCell>Permissions</TableCell>
                      <TableCell>Created</TableCell>
                      <TableCell>Last used</TableCell>
                      <TableCell align="center">Actions</TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {pagedKeys.map((key) => (
                      <TableRow key={key.id}>
                        <DataTableCell>{key.name}</DataTableCell>
                        <DataTableCell sx={{ minWidth: 240 }}>
                          <PermissionPills permissions={key.permissions} />
                        </DataTableCell>
                        <DataTableCell>{formatTimestamp(key.created_at)}</DataTableCell>
                        <DataTableCell>{formatTimestamp(key.last_used)}</DataTableCell>
                        <DataTableCell align="center">
                          <RevokeAgentTokenDialog onConfirm={() => revokeToken(key.id)} />
                        </DataTableCell>
                      </TableRow>
                    ))}

                    {emptyRows > 0 && (
                      <TableRow style={{ height: 57 * emptyRows }}>
                        <TableCell colSpan={5} />
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
                <TablePagination
                  rowsPerPageOptions={[10, 25, 50]}
                  component="div"
                  count={keys.length}
                  rowsPerPage={rowsPerPage}
                  page={page}
                  onPageChange={(_event, newPage) => setPage(newPage)}
                  onRowsPerPageChange={(event) => {
                    setRowsPerPage(parseInt(event.target.value, 10));
                    setPage(0);
                  }}
                />
              </TableContainer>
            )}
          </Box>
        </Fade>
      </FetchStatusCheck>
    </>
  );
}
