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

import { useAccountsList } from "@api/hooks/useAccounts";
import { useSubmitManifest } from "@api/hooks/useTransactions";
import PageHeading from "@components/PageHeading";
import { DataTableCell, StyledPaper } from "@components/StyledComponents";
import {
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  FormControl,
  IconButton,
  InputLabel,
  MenuItem,
  Select,
  Stack,
  Tab,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  Tabs,
  TextareaAutosize,
  useTheme,
} from "@mui/material";
import type { SelectChangeEvent } from "@mui/material";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Grid from "@mui/material/Grid";
import TextField from "@mui/material/TextField";
import useManifestCodeStore from "@store/manifestStore";
import type { ManifestTab } from "@store/manifestStore";
import { rejectReasonToString, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useRef, useState } from "react";

function Manifest() {
  return (
    <>
      <Grid size={12}>
        <PageHeading>Manifest</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <ManifestEditor />
        </StyledPaper>
      </Grid>
    </>
  );
}

function ManifestEditor() {
  const manifest = useManifestCodeStore();
  const [fee, setFee] = useState<bigint>(0n);
  const [finalizeError, setFinalizeError] = useState<string | null>(null);
  const theme = useTheme();

  const { mutateAsync: submitManifest, isPending: isSubmittingManifest, error } = useSubmitManifest();

  const isDryRun = !fee;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    console.log("Manifest code submitted:", manifest.code);
    console.log("Fee submitted:", fee);
    submitManifest({
      manifest: manifest.code,
      variables: manifest.variables,
      max_fee: isDryRun ? 3000 : Number(fee),
      signing_key_id: null,
      dry_run: isDryRun,
    })
      .then((response) => {
        if (!isDryRun) {
          console.log("Manifest submitted successfully:", response);
          return;
        }
        const finalize = response.result?.finalize;
        if (!finalize && isDryRun) {
          throw new Error("No result returned for dry run");
        }
        if ("Accept" in finalize!.result) {
          setFee(BigInt(finalize!.fee_receipt.total_fees_paid));
          setFinalizeError(null);
          console.log("Dry run successful:", finalize);
        } else if ("Reject" in finalize!.result) {
          setFinalizeError(rejectReasonToString(finalize!.result.Reject));
        } else if ("AcceptFeeRejectRest" in finalize!.result) {
          setFinalizeError(rejectReasonToString(finalize!.result.AcceptFeeRejectRest[1]));
        }
      })
      .catch((error) => {
        console.error("Error submitting manifest:", error);
      });
  };

  return (
    <>
      <Grid size={12}>
        <form onSubmit={handleSubmit}>
          <ManifestTabBar
            tabs={manifest.tabs}
            activeTabId={manifest.activeTabId}
            onSelect={manifest.setActiveTab}
            onAdd={manifest.addTab}
            onRemove={manifest.removeTab}
            onRename={manifest.renameTab}
          />
          <TextareaAutosize
            minRows={25}
            aria-label="Manifest code editor"
            name="manifest-code"
            value={manifest.code}
            onChange={(e) => manifest.setCode(e.target.value)}
            style={{
              width: "100%",
              borderRadius: "0 0 8px 8px",
              padding: "8px 32px",
              fontFamily: "monospace",
              backgroundColor: theme.palette.accent.background,
              color: theme.palette.text.primary,
            }}
          />
          <Box className="flex-container" style={{ justifyContent: "flex-start" }}>
            <VariableEditor
              variables={manifest.variables}
              onAdd={manifest.addVariable}
              onRemove={manifest.removeVariable}
              onRename={manifest.renameVariable}
            />
          </Box>
          <Box className="flex-container" style={{ justifyContent: "flex-end" }}>
            <TextField
              name="max-fee"
              placeholder="Max fee"
              value={fee}
              onChange={(e) => setFee(BigInt(e.target.value))}
              type="number"
            />
            <Button type="submit" variant="contained" color="primary">
              {isSubmittingManifest ? "Submitting..." : fee ? "Submit" : "Estimate fee"}
            </Button>
          </Box>
          {error && (
            <Box sx={{ color: "red" }}>
              <p>{error.message}</p>
            </Box>
          )}
          {finalizeError && (
            <Box sx={{ color: "red" }}>
              <p>{finalizeError}</p>
            </Box>
          )}
        </form>
      </Grid>
    </>
  );
}

function ManifestTabBar({
  tabs,
  activeTabId,
  onSelect,
  onAdd,
  onRemove,
  onRename,
}: {
  tabs: ManifestTab[];
  activeTabId: string;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onRemove: (id: string) => void;
  onRename: (id: string, name: string) => void;
}) {
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const tabToDelete = confirmDeleteId ? tabs.find((t) => t.id === confirmDeleteId) : null;

  return (
    <Box sx={{ display: "flex", alignItems: "center", borderBottom: 1, borderColor: "divider" }}>
      <Tabs
        value={activeTabId}
        onChange={(_, id) => onSelect(id)}
        variant="scrollable"
        scrollButtons="auto"
        sx={{ flexGrow: 1 }}
      >
        {tabs.map((tab) => (
          <Tab
            key={tab.id}
            value={tab.id}
            label={
              <TabLabel
                tab={tab}
                isActive={tab.id === activeTabId}
                canClose={tabs.length > 1}
                onClose={() => setConfirmDeleteId(tab.id)}
                onRename={(name) => onRename(tab.id, name)}
              />
            }
            sx={{ textTransform: "none", minHeight: 42, py: 0 }}
          />
        ))}
      </Tabs>
      <IconButton size="small" onClick={onAdd} title="New tab" sx={{ ml: 0.5, mr: 1 }}>
        +
      </IconButton>

      <Dialog open={!!confirmDeleteId} onClose={() => setConfirmDeleteId(null)}>
        <DialogTitle>Delete tab</DialogTitle>
        <DialogContent>
          <DialogContentText>
            Delete &quot;{tabToDelete?.name}&quot;? Any unsaved manifest code in this tab will be lost.
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmDeleteId(null)}>Cancel</Button>
          <Button
            color="error"
            variant="contained"
            onClick={() => {
              onRemove(confirmDeleteId!);
              setConfirmDeleteId(null);
            }}
          >
            Delete Tab
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}

function TabLabel({
  tab,
  isActive,
  canClose,
  onClose,
  onRename,
}: {
  tab: ManifestTab;
  isActive: boolean;
  canClose: boolean;
  onClose: () => void;
  onRename: (name: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(tab.name);

  const commit = () => {
    setEditing(false);
    const trimmed = draft.trim();
    if (trimmed && trimmed !== tab.name) {
      onRename(trimmed);
    } else {
      setDraft(tab.name);
    }
  };

  if (editing) {
    return (
      <TextField
        size="small"
        variant="standard"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") {
            setDraft(tab.name);
            setEditing(false);
          }
        }}
        onClick={(e) => e.stopPropagation()}
        autoFocus
        sx={{ minWidth: 80, maxWidth: 160 }}
        inputProps={{ style: { fontSize: "0.875rem", padding: "2px 0" } }}
      />
    );
  }

  return (
    <Stack direction="row" alignItems="center" spacing={0.5}>
      <Box
        component="span"
        onDoubleClick={(e) => {
          if (isActive) {
            e.stopPropagation();
            setDraft(tab.name);
            setEditing(true);
          }
        }}
      >
        {tab.name}
      </Box>
      {canClose && (
        <Box
          component="span"
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
          sx={{
            ml: 0.5,
            px: 0.5,
            lineHeight: 1,
            fontSize: "1rem",
            cursor: "pointer",
            borderRadius: "50%",
            "&:hover": { backgroundColor: "action.hover" },
          }}
        >
          &times;
        </Box>
      )}
    </Stack>
  );
}

function nextAccountVarName(variables: Record<string, string>): string {
  let i = 1;
  while (`account_${i}` in variables) {
    i++;
  }
  return `account_${i}`;
}

function EditableKeyCell({ varKey, onRename }: { varKey: string; onRename: (oldKey: string, newKey: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(varKey);

  const commit = () => {
    setEditing(false);
    const trimmed = draft.trim();
    if (trimmed && trimmed !== varKey) {
      onRename(varKey, trimmed);
    } else {
      setDraft(varKey);
    }
  };

  if (editing) {
    return (
      <TextField
        size="small"
        variant="standard"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") {
            setDraft(varKey);
            setEditing(false);
          }
        }}
        autoFocus
        sx={{ minWidth: 120 }}
      />
    );
  }

  return (
    <Box onClick={() => setEditing(true)} sx={{ cursor: "pointer", "&:hover": { textDecoration: "underline" } }}>
      {varKey}
    </Box>
  );
}

function VariableEditor({
  variables,
  onAdd,
  onRemove,
  onRename,
}: {
  variables: Record<string, string>;
  onAdd: (key: string, value: string) => void;
  onRemove: (key: string) => void;
  onRename: (oldKey: string, newKey: string) => void;
}) {
  const [showInputs, setShowInputs] = useState(false);
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const keyRef = useRef<HTMLInputElement>(null);
  const { data: accountsData } = useAccountsList(0, 100);

  const handleAddAccount = (e: SelectChangeEvent) => {
    const address = e.target.value;
    if (!address) return;
    const varName = nextAccountVarName(variables);
    onAdd(varName, address);
  };

  const handleAdd = () => {
    if (!key.trim()) return;
    onAdd(key, value);
    setKey("");
    setValue("");
    setShowInputs(false);
  };

  return (
    <Grid size={12} mt={2}>
      {Object.keys(variables).length > 0 && (
        <Table
          sx={{
            marginBottom: 2,
          }}
        >
          <TableHead>
            <TableRow>
              <TableCell>Key</TableCell>
              <TableCell>Value</TableCell>
              <TableCell />
            </TableRow>
          </TableHead>
          <TableBody>
            {Object.entries(variables).map(([k, v]) => (
              <TableRow key={k}>
                <DataTableCell>
                  <EditableKeyCell varKey={k} onRename={onRename} />
                </DataTableCell>
                <DataTableCell>{v}</DataTableCell>
                <DataTableCell>
                  <IconButton size="small" onClick={() => onRemove(k)} title="Remove variable">
                    &times;
                  </IconButton>
                </DataTableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      {showInputs ? (
        <Stack direction="row" spacing={1} alignItems="center" marginBottom={2}>
          <TextField
            name="variable-key"
            placeholder="Variable key"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleAdd();
              if (e.key === "Escape") {
                setShowInputs(false);
                setKey("");
                setValue("");
              }
            }}
            inputRef={keyRef}
            autoFocus
          />
          <TextField
            name="variable-value"
            placeholder="Variable value"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleAdd();
              if (e.key === "Escape") {
                setShowInputs(false);
                setKey("");
                setValue("");
              }
            }}
          />
          <Button
            variant="contained"
            color="primary"
            onClick={handleAdd}
            style={{ minHeight: "52px" }}
          >
            Add
          </Button>
          <Button
            variant="outlined"
            onClick={() => {
              setShowInputs(false);
              setKey("");
              setValue("");
            }}
            style={{ minHeight: "52px" }}
          >
            Cancel
          </Button>
        </Stack>
      ) : (
        <Stack direction="row" spacing={1} alignItems="center" marginBottom={2}>
          <IconButton size="small" onClick={() => setShowInputs(true)} title="Add variable">
            +
          </IconButton>
          {accountsData?.accounts && accountsData.accounts.length > 0 && (
            <FormControl style={{ minWidth: "200px" }}>
              <InputLabel id="add-account-label">Add Account</InputLabel>
              <Select labelId="add-account-label" label="Add Account" value="" onChange={handleAddAccount}>
                {accountsData.accounts.map(({ account }) => {
                  const address = substateIdToString(account.component_address);
                  return (
                    <MenuItem key={address} value={address}>
                      {account.name || address}
                    </MenuItem>
                  );
                })}
              </Select>
            </FormControl>
          )}
        </Stack>
      )}
    </Grid>
  );
}

export default Manifest;
