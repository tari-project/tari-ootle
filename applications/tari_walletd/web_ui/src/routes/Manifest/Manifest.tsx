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
  useTheme,
} from "@mui/material";
import type { SelectChangeEvent } from "@mui/material";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Grid from "@mui/material/Grid";
import TextField from "@mui/material/TextField";
import useManifestCodeStore from "@store/manifestStore";
import type { ManifestTab } from "@store/manifestStore";
import { FileDownload, FileUpload, FormatAlignLeft, LibraryAdd } from "@mui/icons-material";
import type { KeyId } from "@tari-project/ootle-ts-bindings";
import { rejectReasonToString, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useListTemplatesAuthored } from "@api/hooks/useTemplatesAuthored";
import { Highlight, themes } from "prism-react-renderer";
import { useCallback, useRef, useState } from "react";
// eslint-disable-next-line @typescript-eslint/no-explicit-any
import EditorImport from "react-simple-code-editor";
// Vite 8 dev pre-bundler doesn't unwrap exports.default for CJS packages
const Editor = (EditorImport as any).default ?? EditorImport;

function formatManifestCode(code: string): string {
  const lines = code.split("\n");
  const formatted: string[] = [];
  let indent = 0;

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();
    if (!trimmed) {
      formatted.push("");
      continue;
    }

    // Decrease indent for closing braces
    if (trimmed.startsWith("}")) {
      indent = Math.max(0, indent - 1);
    }

    formatted.push("    ".repeat(indent) + trimmed);

    // Increase indent after opening braces (that aren't closed on the same line)
    const opens = (trimmed.match(/{/g) || []).length;
    const closes = (trimmed.match(/}/g) || []).length;
    indent = Math.max(0, indent + opens - closes);
  }

  return formatted.join("\n");
}

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
  const fileInputRef = useRef<HTMLInputElement>(null);

  const isDryRun = !fee;

  const handleSave = useCallback(() => {
    const data = manifest.tabs.map(({ name, code, variables, signingKeys }) => ({
      name,
      code,
      variables,
      signingKeys,
    }));
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "manifest.json";
    a.click();
    URL.revokeObjectURL(url);
  }, [manifest.tabs]);

  const handleLoad = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      const reader = new FileReader();
      reader.onload = () => {
        try {
          const parsed = JSON.parse(reader.result as string);
          if (!Array.isArray(parsed) || parsed.length === 0) {
            alert("Invalid manifest file: expected a non-empty array of tabs.");
            return;
          }
          for (const tab of parsed) {
            if (typeof tab.code !== "string" || typeof tab.name !== "string") {
              alert("Invalid manifest file: each tab must have a name and code.");
              return;
            }
          }
          manifest.loadTabs(parsed);
        } catch {
          alert("Failed to parse manifest file.");
        }
      };
      reader.readAsText(file);
      // Reset so the same file can be loaded again
      e.target.value = "";
    },
    [manifest.loadTabs],
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    console.log("Manifest code submitted:", manifest.code);
    console.log("Fee submitted:", fee);
    submitManifest({
      manifest: manifest.code,
      variables: manifest.variables,
      max_fee: isDryRun ? 3000 : Number(fee),
      seal_signer_key_id: null,
      signing_key_ids: manifest.signingKeys,
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
          setFee(BigInt(response.required_fees!));
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
        <input
          type="file"
          accept=".json"
          ref={fileInputRef}
          onChange={handleFileChange}
          style={{ display: "none" }}
        />
        <form onSubmit={handleSubmit}>
          <ManifestTabBar
            tabs={manifest.tabs}
            activeTabId={manifest.activeTabId}
            onSelect={manifest.setActiveTab}
            onAdd={manifest.addTab}
            onRemove={manifest.removeTab}
            onRename={manifest.renameTab}
            onFormat={() => manifest.setCode(formatManifestCode(manifest.code))}
            onSave={handleSave}
            onLoad={handleLoad}
            onImportTemplate={(address, name) => {
              const importLine = `use template_${address} as ${name};`;
              if (manifest.code.includes(importLine)) return;
              // Insert after any existing use statements, or at the top
              const lines = manifest.code.split("\n");
              let lastUseIdx = -1;
              for (let i = 0; i < lines.length; i++) {
                if (lines[i].trimStart().startsWith("use ")) lastUseIdx = i;
              }
              lines.splice(lastUseIdx + 1, 0, importLine);
              manifest.setCode(lines.join("\n"));
            }}
          />
          <Editor
            value={manifest.code}
            onValueChange={manifest.setCode}
            highlight={(code: string) => (
              <Highlight
                theme={theme.palette.mode === "dark" ? themes.vsDark : themes.vsLight}
                code={code}
                language="rust"
              >
                {({ tokens, getTokenProps }) =>
                  tokens.map((line, i) => (
                    <span key={i}>
                      {line.map((token, key) => (
                        <span key={key} {...getTokenProps({ token })} />
                      ))}
                      {i < tokens.length - 1 ? "\n" : ""}
                    </span>
                  ))
                }
              </Highlight>
            )}
            padding={32}
            style={{
              width: "100%",
              borderRadius: "0 0 8px 8px",
              fontFamily: "'Fira Code', 'Fira Mono', Consolas, Menlo, monospace",
              fontSize: 14,
              backgroundColor: theme.palette.accent.background,
              color: theme.palette.text.primary,
              minHeight: 400,
            }}
            textareaClassName="manifest-code-textarea"
          />
          <Box className="flex-container" style={{ justifyContent: "flex-start" }}>
            <VariableEditor
              variables={manifest.variables}
              onAdd={manifest.addVariable}
              onRemove={manifest.removeVariable}
              onRename={manifest.renameVariable}
              onAddSigningKey={manifest.addSigningKey}
            />
          </Box>
          <Box className="flex-container" style={{ justifyContent: "flex-start" }}>
            <SigningKeysEditor
              signingKeys={manifest.signingKeys}
              onAdd={manifest.addSigningKey}
              onRemove={manifest.removeSigningKey}
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
  onFormat,
  onSave,
  onLoad,
  onImportTemplate,
}: {
  tabs: ManifestTab[];
  activeTabId: string;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onRemove: (id: string) => void;
  onRename: (id: string, name: string) => void;
  onFormat: () => void;
  onSave: () => void;
  onLoad: () => void;
  onImportTemplate: (address: string, name: string) => void;
}) {
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
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
      <IconButton size="small" onClick={onAdd} title="New tab" sx={{ ml: 0.5 }}>
        +
      </IconButton>
      <IconButton size="small" onClick={onFormat} title="Format code">
        <FormatAlignLeft fontSize="small" />
      </IconButton>
      <IconButton size="small" onClick={() => setImportOpen(true)} title="Import template">
        <LibraryAdd fontSize="small" />
      </IconButton>
      <IconButton size="small" onClick={onSave} title="Save manifests to file">
        <FileDownload fontSize="small" />
      </IconButton>
      <IconButton size="small" onClick={onLoad} title="Load manifests from file" sx={{ mr: 1 }}>
        <FileUpload fontSize="small" />
      </IconButton>

      <ImportTemplateDialog
        open={importOpen}
        onClose={() => setImportOpen(false)}
        onImport={(address, name) => {
          onImportTemplate(address, name);
          setImportOpen(false);
        }}
      />

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

function ImportTemplateDialog({
  open,
  onClose,
  onImport,
}: {
  open: boolean;
  onClose: () => void;
  onImport: (address: string, name: string) => void;
}) {
  const { data, isLoading } = useListTemplatesAuthored({
    author_public_key: null,
    page: 0,
    page_size: 100,
  });

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>Import Template</DialogTitle>
      <DialogContent>
        {isLoading && <DialogContentText>Loading templates...</DialogContentText>}
        {!isLoading && (!data?.templates || data.templates.length === 0) && (
          <DialogContentText>No templates found.</DialogContentText>
        )}
        {data?.templates && data.templates.length > 0 && (
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>Name</TableCell>
                <TableCell>Address</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {data.templates.map((t) => (
                <TableRow key={t.address} hover sx={{ cursor: "pointer" }} onClick={() => onImport(t.address, t.name)}>
                  <TableCell>{t.name}</TableCell>
                  <TableCell sx={{ fontFamily: "monospace", fontSize: "0.75rem" }}>
                    {t.address.slice(0, 8)}...{t.address.slice(-8)}
                  </TableCell>
                  <TableCell>
                    <Button size="small" variant="outlined">
                      Import
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
      </DialogActions>
    </Dialog>
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

function EditableValueCell({ varKey, value, onUpdate }: { varKey: string; value: string; onUpdate: (key: string, value: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);

  const commit = () => {
    setEditing(false);
    if (draft !== value) {
      onUpdate(varKey, draft);
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
            setDraft(value);
            setEditing(false);
          }
        }}
        autoFocus
        fullWidth
        sx={{ minWidth: 120 }}
      />
    );
  }

  return (
    <Box onClick={() => { setDraft(value); setEditing(true); }} sx={{ cursor: "pointer", "&:hover": { textDecoration: "underline" } }}>
      {value}
    </Box>
  );
}

function VariableEditor({
  variables,
  onAdd,
  onRemove,
  onRename,
  onAddSigningKey,
}: {
  variables: Record<string, string>;
  onAdd: (key: string, value: string) => void;
  onRemove: (key: string) => void;
  onRename: (oldKey: string, newKey: string) => void;
  onAddSigningKey: (key: KeyId) => void;
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
    // Auto-add the account's signing key
    const accountInfo = accountsData?.accounts.find(
      (a) => substateIdToString(a.account.component_address) === address,
    );
    if (accountInfo?.account.owner_key_id) {
      onAddSigningKey(accountInfo.account.owner_key_id);
    }
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
                <DataTableCell>
                  <EditableValueCell varKey={k} value={v} onUpdate={onAdd} />
                </DataTableCell>
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

function formatKeyId(key: KeyId): string {
  if ("Derived" in key) {
    return `${key.Derived.key_branch}/${key.Derived.index}`;
  }
  return `imported/${key.Imported.local_key_id}`;
}

function findAccountNameForKey(key: KeyId, accounts: { account: { name: string | null; owner_key_id: KeyId | null } }[]): string | null {
  const match = accounts.find(
    (a) => a.account.owner_key_id && JSON.stringify(a.account.owner_key_id) === JSON.stringify(key),
  );
  return match?.account.name || null;
}

function SigningKeysEditor({
  signingKeys,
  onAdd,
  onRemove,
}: {
  signingKeys: KeyId[];
  onAdd: (key: KeyId) => void;
  onRemove: (index: number) => void;
}) {
  const { data: accountsData } = useAccountsList(0, 100);
  const accounts = accountsData?.accounts || [];

  const handleAddAccountKey = (e: SelectChangeEvent) => {
    const address = e.target.value;
    if (!address) return;
    const accountInfo = accounts.find(
      (a) => substateIdToString(a.account.component_address) === address,
    );
    if (accountInfo?.account.owner_key_id) {
      onAdd(accountInfo.account.owner_key_id);
    }
  };

  return (
    <Grid size={12} mt={1}>
      {signingKeys.length > 0 && (
        <Table sx={{ marginBottom: 2 }}>
          <TableHead>
            <TableRow>
              <TableCell>Signing Keys</TableCell>
              <TableCell>Account</TableCell>
              <TableCell />
            </TableRow>
          </TableHead>
          <TableBody>
            {signingKeys.map((key, index) => {
              const accountName = findAccountNameForKey(key, accounts);
              return (
                <TableRow key={index}>
                  <DataTableCell sx={{ fontFamily: "monospace", fontSize: "0.8rem" }}>
                    {formatKeyId(key)}
                  </DataTableCell>
                  <DataTableCell>
                    {accountName || "-"}
                  </DataTableCell>
                  <DataTableCell>
                    <IconButton size="small" onClick={() => onRemove(index)} title="Remove signing key">
                      &times;
                    </IconButton>
                  </DataTableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      <Stack direction="row" spacing={1} alignItems="center" marginBottom={2}>
        {accounts.length > 0 && (
          <FormControl style={{ minWidth: "200px" }}>
            <InputLabel id="add-signing-key-label">Add Signing Key</InputLabel>
            <Select labelId="add-signing-key-label" label="Add Signing Key" value="" onChange={handleAddAccountKey}>
              {accounts
                .filter((a) => a.account.owner_key_id)
                .map(({ account }) => {
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
    </Grid>
  );
}

export default Manifest;
