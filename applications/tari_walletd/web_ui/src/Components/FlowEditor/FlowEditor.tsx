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

import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import { QueryBuilder, TemplateReader, useStore } from "@tari-project/tari-extension-query-builder";
import "@tari-project/tari-extension-query-builder/dist/tari-extension-query-builder.css";
import useThemeStore from "../../store/themeStore";
import { useCallback, useState } from "react";
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
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
} from "@mui/material";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import { useTheme } from "@mui/material/styles";
import Loading from "../Loading";
import { useTemplateGet } from "../../api/hooks/useTemplate";
import AddCircleOutlineIcon from "@mui/icons-material/AddCircleOutline";
import FunctionsIcon from "@mui/icons-material/Functions";
import SettingsEthernetIcon from "@mui/icons-material/SettingsEthernet";
import useAccountStore from "../../store/accountStore";
import { GeneratedCodeType, TransactionProps } from "@tari-project/tari-extension-common";
import { substateIdToString } from "../../utils/helpers";
import CloseIcon from "@mui/icons-material/Close";
import { Highlight } from "prism-react-renderer";

function FlowEditor() {
  const { themeMode } = useThemeStore();
  const [panelOpen, setPanelOpen] = useState(true);
  const [componentId, setComponentId] = useState("");
  const [codeDialogOpen, setCodeDialogOpen] = useState(false);
  const [generatedCode, setGeneratedCode] = useState("");
  const [generatedCodeType, setGeneratedCodeType] = useState<GeneratedCodeType | null>(null);
  const theme = useTheme();
  const { account } = useAccountStore();
  const addNodeAt = useStore((store) => store.addNodeAt);

  const { data, isLoading, error, refetch } = useTemplateGet(
    { template_address: componentId },
    { enabled: !!componentId },
  );

  const templateDef = data?.template_definition;
  const methods = templateDef?.V1.functions || [];

  const addNodesToCenter = useCallback(
    (functionName: string) => {
      if (componentId && templateDef) {
        const reader = new TemplateReader(templateDef, componentId);
        const nodeData = reader.getGenericNode(functionName);
        if (nodeData) {
          addNodeAt(nodeData);
        }
      }
    },
    [addNodeAt, componentId, templateDef],
  );

  const getTransactionProps = async (): Promise<TransactionProps> => {
    return {
      accountAddress: account ? substateIdToString(account.address) : "",
      fee: 1000, // TODO:
    };
  };

  const showGeneratedCode = async (code: string, type: GeneratedCodeType): Promise<void> => {
    setGeneratedCode(code);
    setGeneratedCodeType(type);
    setCodeDialogOpen(true);
  };

  const executeTransaction = async (): Promise<void> => {};

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
          <Typography variant="h6">Component Methods</Typography>
          <IconButton onClick={() => setPanelOpen(false)} size="small">
            <ChevronRightIcon />
          </IconButton>
        </Box>
        <Divider />
        <Box mt={2} mb={2}>
          <Typography variant="subtitle2" gutterBottom>
            Component ID
          </Typography>
          <Box display="flex" gap={1}>
            <TextField
              size="small"
              value={componentId}
              onChange={(e) => setComponentId(e.target.value)}
              placeholder="Enter component id"
              fullWidth
            />
            <Button variant="contained" onClick={() => refetch()} disabled={!componentId || isLoading}>
              Fetch
            </Button>
          </Box>
        </Box>
        {componentId && isLoading && <Loading />}
        {componentId && error && (
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
                    templateAddress: componentId,
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
