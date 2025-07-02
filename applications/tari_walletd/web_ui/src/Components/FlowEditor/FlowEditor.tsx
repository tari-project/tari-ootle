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

import "@tari-project/tari-extension-query-builder/dist/tari-extension-query-builder.css";
import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import { QueryBuilder, TemplateReader, useStore } from "@tari-project/tari-extension-query-builder";
import useThemeStore from "../../store/themeStore";
import { useCallback, useEffect, useRef } from "react";
import {
  Button,
  TextField,
  Typography,
  IconButton,
  List,
  ListItem,
  Divider,
  Drawer,
  Box,
  Tooltip,
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  SelectChangeEvent,
} from "@mui/material";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import { useTheme } from "@mui/material/styles";
import Loading from "../Loading";
import { useTemplateGet } from "../../api/hooks/useTemplate";
import AddCircleOutlineIcon from "@mui/icons-material/AddCircleOutline";
import FunctionsIcon from "@mui/icons-material/Functions";
import SettingsEthernetIcon from "@mui/icons-material/SettingsEthernet";
import { GeneratedCodeType, TariNetwork, TransactionProps } from "@tari-project/tari-extension-common";
import { substateIdToString } from "../../utils/helpers";
import CloseIcon from "@mui/icons-material/Close";
import { Highlight } from "prism-react-renderer";
import useFlowEditorStore from "../../store/flowEditorStore";
import { UnsignedTransactionV1 } from "@tari-project/typescript-bindings";
import { settingsGet, submitTransactionDryRun, transactionsSubmit, transactionsWaitResult } from "../../utils/json_rpc";
import { useAccountsList } from "../../api/hooks/useAccounts";

enum Network {
  MainNet = 0,
  StageNet = 1,
  NextNet = 2,
  LocalNet = 16,
  Igor = 36,
  Esmeralda = 38,
}
enum TransactionStatus {
  New = "New",
  DryRun = "DryRun",
  Pending = "Pending",
  Accepted = "Accepted",
  Rejected = "Rejected",
  InvalidTransaction = "InvalidTransaction",
  OnlyFeeAccepted = "OnlyFeeAccepted",
}

function FlowEditor() {
  const { themeMode } = useThemeStore();
  const {
    panelOpen,
    setPanelOpen,
    templateId,
    setTemplateId,
    codeDialogOpen,
    setCodeDialogOpen,
    generatedCode,
    setGeneratedCode,
    generatedCodeType,
    setGeneratedCodeType,
    account,
    setAccount,
    fee,
    setFee,
  } = useFlowEditorStore();
  const theme = useTheme();
  const addNodeAt = useStore((store) => store.addNodeAt);
  const saveStateToString = useStore((store) => store.saveStateToString);
  const loadStateFromString = useStore((store) => store.loadStateFromString);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const { data: dataAccountsList } = useAccountsList(0, 20);

  const { data, isLoading, error, refetch } = useTemplateGet(
    { template_address: templateId },
    { enabled: !!templateId },
  );

  const templateDef = data?.template_definition;
  const methods = templateDef?.V1.functions || [];

  const addNodesToCenter = useCallback(
    (functionName: string) => {
      if (templateId && templateDef) {
        const reader = new TemplateReader(templateDef, templateId);
        const nodeData = reader.getGenericNode(functionName);
        if (nodeData) {
          addNodeAt(nodeData);
        }
      }
    },
    [addNodeAt, templateId, templateDef],
  );

  const getTariNetwork = async (): Promise<TariNetwork> => {
    const settings = await settingsGet();
    switch (settings.network.byte) {
      case Network.MainNet:
        return TariNetwork.MainNet;
      case Network.StageNet:
        return TariNetwork.StageNet;
      case Network.NextNet:
        return TariNetwork.NextNet;
      case Network.LocalNet:
        return TariNetwork.LocalNet;
      case Network.Igor:
        return TariNetwork.Igor;
      case Network.Esmeralda:
        return TariNetwork.Esmeralda;
      default:
        return TariNetwork.LocalNet;
    }
  };

  useEffect(() => {
    if (dataAccountsList?.accounts && dataAccountsList.accounts.length > 0) {
      const defaultAcc = dataAccountsList.accounts.find((acc) => acc.account.is_default);
      setAccount(defaultAcc || dataAccountsList.accounts[0]);
    }
  }, [dataAccountsList]);

  const onAccountChange = (e: SelectChangeEvent<string>) => {
    const selected = dataAccountsList?.accounts.find(
      (acc) => substateIdToString(acc.account.address) === e.target.value,
    );
    setAccount(selected);
  };

  const getTransactionProps = async (): Promise<TransactionProps> => {
    if (!account) {
      throw new Error("Account is not available");
    }
    const network = await getTariNetwork();
    const accountAddress = substateIdToString(account.account.address);
    return {
      network,
      accountAddress,
      fee,
    };
  };

  const showGeneratedCode = async (code: string, type: GeneratedCodeType): Promise<void> => {
    setGeneratedCode(code);
    setGeneratedCodeType(type);
    setCodeDialogOpen(true);
  };

  const executeTransaction = async (transaction: UnsignedTransactionV1): Promise<void> => {
    if (!account) {
      throw new Error("Account is not available");
    }
    const request = {
      transaction: { V1: transaction },
      signing_key_index: account.account.key_index,
      detect_inputs: true,
      detect_inputs_use_unversioned: true,
      proof_ids: [],
    };
    const submitResp = transaction.dry_run ? await submitTransactionDryRun(request) : await transactionsSubmit(request);
    const result = await transactionsWaitResult({
      transaction_id: submitResp.transaction_id,
      timeout_secs: 60,
    });
    const txResult = result.result?.result;
    const success = txResult && "Accept" in txResult;
    if (success) {
      if (transaction.dry_run) {
        setFee(result.final_fee);
      }
    } else {
      let failureReason = undefined;
      if (!txResult) {
        failureReason = "Execution failed";
      } else if ("Reject" in txResult) {
        failureReason = JSON.stringify(txResult.Reject);
      } else {
        failureReason = JSON.stringify(txResult.AcceptFeeRejectRest[1]);
      }
      throw new Error(failureReason);
    }
  };

  const handleSaveFlow = () => {
    const json = saveStateToString();
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "flow.tari";
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleLoadFlow = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (event) => {
      try {
        loadStateFromString(event.target?.result as string);
      } catch (err) {
        alert("Failed to load flow: " + err);
      }
    };
    reader.readAsText(file);
  };

  return (
    <Grid container spacing={2}>
      <Grid item xs={panelOpen ? 9 : 12}>
        <Grid item xs={12} md={12} lg={12}>
          <PageHeading>Flow Editor</PageHeading>
        </Grid>
        <Grid item xs={12} md={12} lg={12}>
          <StyledPaper>
            <div style={{ height: "600px", width: "100%" }}>
              <QueryBuilder
                theme={themeMode}
                getTransactionProps={getTransactionProps}
                showGeneratedCode={showGeneratedCode}
                executeTransaction={executeTransaction}
              />
            </div>
          </StyledPaper>
        </Grid>
      </Grid>
      <Drawer
        variant="persistent"
        anchor="right"
        open={panelOpen}
        PaperProps={{
          sx: {
            width: 340,
            p: 2,
            top: 64, // height of the toolbar
            height: "calc(100% - 64px)",
            position: "fixed",
          },
        }}
      >
        <Box display="flex" alignItems="center" justifyContent="space-between" mb={2}>
          <Typography variant="h6">Details</Typography>
          <IconButton onClick={() => setPanelOpen(false)} size="small">
            <ChevronRightIcon />
          </IconButton>
        </Box>
        <Box mb={2}>
          <FormControl fullWidth size="small">
            <InputLabel id="account-select-label">Account</InputLabel>
            <Select
              labelId="account-select-label"
              name="account"
              label="Account"
              value={account ? substateIdToString(account.account.address) : ""}
              onChange={onAccountChange}
            >
              {dataAccountsList?.accounts?.map((acc) => (
                <MenuItem key={substateIdToString(acc.account.address)} value={substateIdToString(acc.account.address)}>
                  {acc.account.name || substateIdToString(acc.account.address)}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
        </Box>
        <Divider />
        <Box mt={2} mb={2}>
          <Typography variant="subtitle2" gutterBottom>
            Transaction Fee
          </Typography>
          <TextField
            size="small"
            type="number"
            value={fee}
            onChange={(e) => setFee(Number(e.target.value))}
            inputProps={{ min: 0 }}
            fullWidth
          />
        </Box>
        <Box mt={2} mb={2} display="flex" gap={1}>
          <Button variant="outlined" onClick={handleSaveFlow} size="small">
            Save Flow
          </Button>
          <Button variant="outlined" onClick={() => fileInputRef.current?.click()} size="small">
            Load Flow
          </Button>
          <input
            type="file"
            accept=".tari,application/json"
            ref={fileInputRef}
            style={{ display: "none" }}
            onChange={handleLoadFlow}
          />
        </Box>
        <Box mt={2} mb={2}>
          <Typography variant="subtitle2" gutterBottom>
            Template ID
          </Typography>
          <Box display="flex" gap={1}>
            <TextField
              size="small"
              value={templateId}
              onChange={(e) => setTemplateId(e.target.value)}
              placeholder="Enter template id"
              fullWidth
            />
            <Button variant="contained" onClick={() => refetch()} disabled={!templateId || isLoading}>
              Fetch
            </Button>
          </Box>
        </Box>
        {templateId && isLoading && <Loading />}
        {templateId && error && (
          <Typography color="error" variant="body2">
            {error.message || "Failed to fetch methods"}
          </Typography>
        )}
        <List>
          {methods.map((m, i) => (
            <ListItem
              key={i}
              sx={{ display: "flex", alignItems: "center", gap: 1, cursor: "grab" }}
              draggable
              onDragStart={(e) => {
                e.dataTransfer.setData(
                  "CALL_NODE_DRAG_DROP_TYPE",
                  JSON.stringify({
                    template: templateDef,
                    templateAddress: templateId,
                    functionName: m.name,
                  }),
                );
              }}
              secondaryAction={
                <IconButton
                  edge="end"
                  onClick={() => {
                    addNodesToCenter(m.name);
                  }}
                >
                  <AddCircleOutlineIcon />
                </IconButton>
              }
            >
              {m.arguments.length && m.arguments[0].name === "self" ? (
                <SettingsEthernetIcon color="secondary" fontSize="small" />
              ) : (
                <FunctionsIcon color="primary" fontSize="small" />
              )}
              <Tooltip title={m.name} placement="top" arrow>
                <Typography
                  variant="body1"
                  sx={{
                    ml: 1,
                    flex: 1,
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {m.name}
                </Typography>
              </Tooltip>
            </ListItem>
          ))}
        </List>
      </Drawer>
      <Dialog open={codeDialogOpen} onClose={() => setCodeDialogOpen(false)} maxWidth="md" fullWidth>
        <DialogTitle>
          {generatedCodeType === GeneratedCodeType.Typescript ? "TypeScript code" : "JavaScript code"}
          <IconButton
            onClick={() => setCodeDialogOpen(false)}
            sx={{ position: "absolute", right: 8, top: 8 }}
            size="small"
          >
            <CloseIcon />
          </IconButton>
        </DialogTitle>
        <DialogContent dividers>
          {generatedCodeType && (
            <Highlight
              code={generatedCode}
              language={generatedCodeType === GeneratedCodeType.Typescript ? "typescript" : "javascript"}
            >
              {({ className, style, tokens, getLineProps, getTokenProps }) => (
                <pre
                  className={className}
                  style={{ ...style, margin: 0, borderRadius: 4, fontSize: 14, overflowX: "auto" }}
                >
                  {tokens.map((line, i) => (
                    <div key={i} {...getLineProps({ line, key: i })}>
                      {line.map((token, key) => (
                        <span key={key} {...getTokenProps({ token, key })} />
                      ))}
                    </div>
                  ))}
                </pre>
              )}
            </Highlight>
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCodeDialogOpen(false)} color="primary">
            Close
          </Button>
        </DialogActions>
      </Dialog>
      {!panelOpen && (
        <Box position="fixed" right={0} top={80} sx={{ zIndex: theme.zIndex.drawer + 1 }}>
          <IconButton onClick={() => setPanelOpen(true)} size="small">
            <ChevronLeftIcon />
          </IconButton>
        </Box>
      )}
    </Grid>
  );
}

export default FlowEditor;
