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

import PopupTitle from "@/components/PopupTitle";
import { useAccountsList } from "@api/hooks/useAccounts";
import { usePublishTemplate } from "@api/hooks/useTransactions";
import HelpOutlineIcon from "@mui/icons-material/HelpOutline";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select, { SelectChangeEvent } from "@mui/material/Select";
import { useTheme } from "@mui/material/styles";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import useAccountStore from "@store/accountStore";
import {
  AccountInfo,
  PublishTemplateResponse,
  ResourceAddress,
  ResourceType,
  substateIdToString,
} from "@tari-project/ootle-ts-bindings";
import { base64FromArrayBuffer } from "@utils/helpers";
import { DragEvent, FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Form } from "react-router-dom";

const MAX_WASM_SIZE = 3 * 1024 * 1024; // 3 MiB

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export default function PublishTemplate() {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button variant="outlined" onClick={() => setOpen(true)}>
        Publish Template
      </Button>
      <PublishTemplateDialog
        open={open}
        handleClose={() => setOpen(false)}
        onSendComplete={() => setOpen(false)}
        resource_type="Confidential"
      />
    </>
  );
}

export interface DialogProps {
  open: boolean;
  resource_address?: ResourceAddress;
  resource_type?: ResourceType;
  onSendComplete?: () => void;
  handleClose: () => void;
}

interface FormState {
  binary: ArrayBuffer | null;
  fileName: string | null;
  fileSize: number | null;
  metadata: ArrayBuffer | null;
  metadataFileName: string | null;
  account: string | null;
  maxFee: string;
}

function PublishTemplateDialog(props: DialogProps) {
  const INITIAL_VALUES: FormState = {
    binary: null,
    fileName: null,
    fileSize: null,
    metadata: null,
    metadataFileName: null,
    account: null,
    maxFee: "",
  };

  const [formState, setFormState] = useState<FormState>(INITIAL_VALUES);
  const [disabled, setDisabled] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const [fileError, setFileError] = useState<string | null>(null);
  const [estimatedFee, setEstimatedFee] = useState<number | null>(null);
  const [isEstimating, setIsEstimating] = useState(false);

  const fileInputRef = useRef<HTMLInputElement>(null);
  const metadataInputRef = useRef<HTMLInputElement>(null);
  const dragCounterRef = useRef(0);

  const { account, setPopup } = useAccountStore();
  const theme = useTheme();

  const { data: accountsResp } = useAccountsList(0, 1000);
  const accounts = accountsResp?.accounts;

  const { mutateAsync: publishTemplate } = usePublishTemplate();

  const hasFile = formState.binary !== null;
  const hasAccount = Boolean(formState.account);
  const maxFeeNum = formState.maxFee ? BigInt(formState.maxFee) : 0n;
  const feeIsBelowEstimate = estimatedFee !== null && maxFeeNum !== null && maxFeeNum > 0 && maxFeeNum < estimatedFee;
  const canSubmit = hasFile && hasAccount && maxFeeNum !== null && maxFeeNum > 0;

  // Set default account when accounts load
  useEffect(() => {
    if (accounts?.length && !formState.account) {
      const defaultAccount = accounts.find((a: AccountInfo) => a.account.is_default);
      if (defaultAccount) {
        setFormState((prev) => ({
          ...prev,
          account: substateIdToString(defaultAccount.account.component_address),
        }));
      }
    }
  }, [accounts]);

  // Set account from store
  useEffect(() => {
    if (account && !formState.account) {
      setFormState((prev) => ({
        ...prev,
        account: substateIdToString(account.component_address),
      }));
    }
  }, [account]);

  // Reset state when dialog opens
  useEffect(() => {
    if (props.open) {
      setFormState({
        ...INITIAL_VALUES,
        account: account ? substateIdToString(account.component_address) : null,
      });
      setEstimatedFee(null);
      setFileError(null);
      setIsEstimating(false);
    }
  }, [props.open]);

  const processFile = useCallback((file: File) => {
    setFileError(null);

    if (!file.name.endsWith(".wasm")) {
      setFileError("Only .wasm files are accepted");
      return;
    }

    if (file.size > MAX_WASM_SIZE) {
      setFileError(
        `File is too large (${formatFileSize(file.size)}). Maximum size is ${formatFileSize(MAX_WASM_SIZE)}`,
      );
      return;
    }

    if (file.size === 0) {
      setFileError("File is empty");
      return;
    }

    const reader = new FileReader();
    reader.onload = () => {
      setFormState((prev) => ({
        ...prev,
        binary: reader.result as ArrayBuffer,
        fileName: file.name,
        fileSize: file.size,
      }));
      setEstimatedFee(null);
    };
    reader.onerror = () => {
      setFileError("Failed to read file");
    };
    reader.readAsArrayBuffer(file);
  }, []);

  const handleDragEnter = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current++;
    if (e.dataTransfer.items?.length) {
      setIsDragging(true);
    }
  };

  const handleDragLeave = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current--;
    if (dragCounterRef.current === 0) {
      setIsDragging(false);
    }
  };

  const handleDragOver = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
    dragCounterRef.current = 0;

    const files = e.dataTransfer.files;
    if (files.length > 0) {
      processFile(files[0]);
    }
  };

  const handleFileInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      processFile(files[0]);
    }
    // Reset so the same file can be re-selected
    e.target.value = "";
  };

  const handleRemoveFile = () => {
    setFormState((prev) => ({
      ...prev,
      binary: null,
      fileName: null,
      fileSize: null,
    }));
    setEstimatedFee(null);
    setFileError(null);
  };

  const handleMetadataFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      const file = files[0];
      const nameLower = file.name.toLowerCase();
      if (!nameLower.endsWith(".json") && !nameLower.endsWith(".cbor")) {
        setFileError("Metadata must be a .json or .cbor file");
        return;
      }
      const reader = new FileReader();
      reader.onload = () => {
        const result = reader.result as ArrayBuffer;
        if (nameLower.endsWith(".json")) {
          try {
            JSON.parse(new TextDecoder().decode(result));
          } catch {
            setFileError("Invalid JSON in metadata file");
            return;
          }
        }
        setFormState((prev) => ({
          ...prev,
          metadata: result,
          metadataFileName: file.name,
        }));
      };
      reader.onerror = () => setFileError("Failed to read metadata file");
      reader.readAsArrayBuffer(file);
    }
    e.target.value = "";
  };

  const handleRemoveMetadata = () => {
    setFormState((prev) => ({ ...prev, metadata: null, metadataFileName: null }));
  };

  const buildMetadataPayload = () => {
    if (!formState.metadata) return null;
    if (formState.metadataFileName?.endsWith(".cbor")) {
      return { type: "RawCbor" as const, data: base64FromArrayBuffer(formState.metadata) };
    }
    // JSON file: parse and send as Literal
    const text = new TextDecoder().decode(formState.metadata);
    return { type: "Literal" as const, data: JSON.parse(text) };
  };

  const handleEstimateFee = async () => {
    if (!formState.binary || !formState.account) return;

    setIsEstimating(true);
    try {
      const resp: PublishTemplateResponse = await publishTemplate({
        fee_account: { ComponentAddress: formState.account },
        binary: base64FromArrayBuffer(formState.binary),
        max_fee: 1n, // Dry run requires no fees
        detect_inputs: true,
        dry_run: true,
        metadata: buildMetadataPayload(),
      });
      const fee = resp.dry_run_fee!;
      setEstimatedFee(fee);
      setFormState((prev) => ({ ...prev, maxFee: String(fee) }));
    } catch (e: unknown) {
      const message = e instanceof Error ? e.message : "Unknown error";
      setPopup({ title: "Fee estimation failed", error: true, message });
    } finally {
      setIsEstimating(false);
    }
  };

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!formState.binary || !formState.account || !maxFeeNum) return;

    setDisabled(true);
    try {
      await publishTemplate({
        fee_account: { ComponentAddress: formState.account },
        binary: base64FromArrayBuffer(formState.binary),
        max_fee: maxFeeNum,
        detect_inputs: true,
        dry_run: false,
        metadata: buildMetadataPayload(),
      });
      setFormState(INITIAL_VALUES);
      setEstimatedFee(null);
      props.onSendComplete?.();
      setPopup({ title: "Publish template transaction submitted", error: false });
    } catch (e: unknown) {
      const message = e instanceof Error ? e.message : "Unknown error";
      setPopup({ title: "Publish failed", error: true, message });
    } finally {
      setDisabled(false);
    }
  };

  const handleClose = () => {
    if (!disabled && !isEstimating) {
      props.handleClose?.();
    }
  };

  const dropZoneBorder = fileError
    ? theme.palette.error.main
    : isDragging
      ? theme.palette.primary.main
      : hasFile
        ? theme.palette.success.main
        : theme.palette.divider;

  const dropZoneBg = isDragging
    ? theme.palette.mode === "dark"
      ? "rgba(144,202,249,0.08)"
      : "rgba(25,118,210,0.04)"
    : "transparent";

  return (
    <Dialog open={props.open} onClose={handleClose} maxWidth="sm" fullWidth>
      <PopupTitle onClose={handleClose} title="Publish Template" />
      <DialogContent sx={{ minWidth: 480, pb: 3 }}>
        <Form onSubmit={onSubmit} className="flex-container-vertical" style={{ paddingTop: theme.spacing(1), gap: 16 }}>
          {/* Account selector */}
          {accounts && (
            <Box>
              <InputLabel id="select-account" sx={{ mb: 0.5 }}>
                Pay Fee From Account
              </InputLabel>
              <Select
                id="select-account"
                name="account"
                disabled={disabled || isEstimating}
                displayEmpty
                value={formState.account || ""}
                onChange={(e: SelectChangeEvent<unknown>) => {
                  setFormState((prev) => ({ ...prev, account: e.target.value as string }));
                }}
                variant="outlined"
                size="small"
                fullWidth
              >
                {accounts.map((acc: AccountInfo, i: number) => (
                  <MenuItem key={i} value={substateIdToString(acc.account.component_address)}>
                    {acc.account.name} {acc.account.is_default ? "(default)" : ""}
                  </MenuItem>
                ))}
              </Select>
            </Box>
          )}

          {/* File drop zone */}
          <Box>
            <InputLabel sx={{ mb: 0.5 }}>WASM Template</InputLabel>
            <Box
              onDragEnter={handleDragEnter}
              onDragLeave={handleDragLeave}
              onDragOver={handleDragOver}
              onDrop={handleDrop}
              onClick={() => !disabled && !isEstimating && fileInputRef.current?.click()}
              sx={{
                "border": `2px dashed ${dropZoneBorder}`,
                "borderRadius": 1,
                "p": 3,
                "textAlign": "center",
                "cursor": disabled || isEstimating ? "default" : "pointer",
                "backgroundColor": dropZoneBg,
                "transition": "border-color 0.2s, background-color 0.2s",
                "&:hover": disabled || isEstimating ? {} : { borderColor: theme.palette.primary.main },
              }}
            >
              <input
                ref={fileInputRef}
                type="file"
                accept=".wasm"
                onChange={handleFileInputChange}
                style={{ display: "none" }}
              />
              {hasFile ? (
                <Box>
                  <Typography variant="body1" sx={{ fontWeight: 500 }}>
                    {formState.fileName}
                  </Typography>
                  <Typography variant="body2" color="text.secondary">
                    {formatFileSize(formState.fileSize!)}
                  </Typography>
                  <Button
                    size="small"
                    color="error"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleRemoveFile();
                    }}
                    disabled={disabled || isEstimating}
                    sx={{ mt: 1 }}
                  >
                    Remove
                  </Button>
                </Box>
              ) : (
                <Box>
                  <Typography variant="body1" color="text.secondary">
                    {isDragging ? "Drop your .wasm file here" : "Drag & drop a .wasm file here, or click to browse"}
                  </Typography>
                  <Typography variant="caption" color="text.secondary">
                    Max size: {formatFileSize(MAX_WASM_SIZE)}
                  </Typography>
                </Box>
              )}
            </Box>
            {fileError && (
              <Typography variant="body2" color="error" sx={{ mt: 0.5 }}>
                {fileError}
              </Typography>
            )}
          </Box>

          {/* Optional metadata file */}
          <Box>
            <Box sx={{ display: "flex", alignItems: "center", gap: 0.5, mb: 0.5 }}>
              <InputLabel>Template Metadata (optional)</InputLabel>
              <Tooltip
                title={
                  <>
                    Optionally attach a JSON or CBOR metadata file describing your template (name, version, description,
                    tags, etc.). A cryptographic hash of this metadata will be published on-chain alongside the
                    template, allowing clients to verify the authenticity of off-chain metadata. This cannot be changed
                    after publishing.
                    <br />
                    <br />
                    When publishing with{" "}
                    <a
                      href="https://crates.io/crates/tari-ootle-cli"
                      target="_blank"
                      rel="noopener noreferrer"
                      style={{ color: "inherit" }}
                    >
                      tari-ootle-cli
                    </a>
                    , this metadata is built automatically from your Cargo.toml.
                  </>
                }
                arrow
                placement="right"
              >
                <HelpOutlineIcon sx={{ fontSize: 16, color: "text.secondary", cursor: "help" }} />
              </Tooltip>
            </Box>
            {formState.metadataFileName ? (
              <Box
                sx={{
                  display: "flex",
                  alignItems: "center",
                  gap: 1,
                  p: 1,
                  border: `1px solid ${theme.palette.success.main}`,
                  borderRadius: 1,
                }}
              >
                <Typography variant="body2" sx={{ flex: 1 }}>
                  {formState.metadataFileName}
                </Typography>
                <Button size="small" color="error" onClick={handleRemoveMetadata} disabled={disabled || isEstimating}>
                  Remove
                </Button>
              </Box>
            ) : (
              <Button
                variant="outlined"
                size="small"
                onClick={() => metadataInputRef.current?.click()}
                disabled={disabled || isEstimating}
              >
                Attach .json or .cbor metadata
              </Button>
            )}
            <input
              ref={metadataInputRef}
              type="file"
              accept=".json,.cbor"
              onChange={handleMetadataFileChange}
              style={{ display: "none" }}
            />
          </Box>

          {/* Fee section */}
          <Box>
            <Box sx={{ display: "flex", alignItems: "center", gap: 1, mb: 0.5 }}>
              <InputLabel>Max Fee</InputLabel>
              {hasFile && hasAccount && (
                <Button
                  size="small"
                  variant="text"
                  onClick={handleEstimateFee}
                  disabled={disabled || isEstimating}
                  sx={{ textTransform: "none", minWidth: 0, p: "2px 8px" }}
                >
                  {isEstimating ? (
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <CircularProgress size={14} />
                      Estimating...
                    </Box>
                  ) : estimatedFee !== null ? (
                    "Re-estimate"
                  ) : (
                    "Estimate fee"
                  )}
                </Button>
              )}
            </Box>
            <TextField
              name="maxFee"
              type="number"
              value={formState.maxFee}
              placeholder={estimatedFee !== null ? `Estimated: ${estimatedFee}` : "Enter max fee or estimate first"}
              onChange={(e) => setFormState((prev) => ({ ...prev, maxFee: e.target.value }))}
              disabled={disabled || isEstimating}
              fullWidth
              size="small"
              slotProps={{ htmlInput: { min: 0, step: "any" } }}
            />
            {feeIsBelowEstimate && (
              <Alert severity="warning" sx={{ mt: 1 }}>
                Fee is below the estimated fee of {estimatedFee}. The transaction will likely be rejected.
              </Alert>
            )}
          </Box>

          {/* Actions */}
          <Box sx={{ display: "flex", justifyContent: "flex-end", gap: 1, mt: 1 }}>
            <Button variant="outlined" onClick={handleClose} disabled={disabled || isEstimating}>
              Cancel
            </Button>
            <Button variant="contained" type="submit" disabled={disabled || isEstimating || !canSubmit}>
              {disabled ? (
                <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                  <CircularProgress size={16} color="inherit" />
                  Publishing...
                </Box>
              ) : (
                "Publish"
              )}
            </Button>
          </Box>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
