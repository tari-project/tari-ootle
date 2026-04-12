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
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select, { SelectChangeEvent } from "@mui/material/Select";
import { useTheme } from "@mui/material/styles";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import useAccountStore from "@store/accountStore";
import type {
  AccountInfo,
  BurnProofFileInfo,
  ClaimBurnProof,
  ClaimBurnProofContents,
  ComponentAddress,
} from "@tari-project/ootle-ts-bindings";
import { rejectReasonToString } from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";
import { formatCurrency } from "@utils/helpers";
import { accountsClaimBurn, burnProofsGet, burnProofsList, transactionsWaitResult } from "@utils/json_rpc";
import { FormEvent, useEffect, useState } from "react";
import { Form } from "react-router";

type ProofSource = "file" | "paste";

interface FormState {
  account: ComponentAddress;
  proofSource: ProofSource;
  selectedFile: string;
  pastedProof: string;
  fee: string;
  disabled: boolean;
}

const INITIAL_FORM_STATE: FormState = {
  account: "",
  proofSource: "file",
  selectedFile: "",
  pastedProof: "",
  fee: "",
  disabled: false,
};

export default function ClaimBurn() {
  const [open, setOpen] = useState(false);
  const [formState, setFormState] = useState<FormState>(INITIAL_FORM_STATE);
  const [burnProofFiles, setBurnProofFiles] = useState<BurnProofFileInfo[]>([]);
  const [isLoadingProofs, setIsLoadingProofs] = useState(false);
  const [isEstimating, setIsEstimating] = useState(false);
  const [proofError, setProofError] = useState<string | null>(null);
  const [proofDetails, setProofDetails] = useState<ClaimBurnProofContents | null>(null);
  const [isLoadingDetails, setIsLoadingDetails] = useState(false);

  const { data: accountsList } = useAccountsList(0, 10);
  const { setPopup } = useAccountStore();
  const theme = useTheme();

  const selectedAccount = accountsList?.accounts?.find(
    (a: AccountInfo) => a.account.component_address === formState.account,
  );

  useEffect(() => {
    if (open) {
      loadBurnProofs(selectedAccount?.account.owner_public_key ?? null);
    }
  }, [open, formState.account]);

  const loadBurnProofs = async (ownerPublicKey: string | null) => {
    setIsLoadingProofs(true);
    try {
      const resp = await burnProofsList({ filter_by_public_key: ownerPublicKey });
      setBurnProofFiles(resp.proofs);
    } catch (e: any) {
      console.warn("Failed to load burn proofs:", e.message);
      setBurnProofFiles([]);
    } finally {
      setIsLoadingProofs(false);
    }
  };

  const truncateHex = (hex: string, chars: number = 8): string =>
    hex.length > chars * 2 + 2 ? `${hex.slice(0, chars)}...${hex.slice(-chars)}` : hex;

  /** Extracts the commitment hex from a filename like `{pubkey}-{commitment}.json` */
  const extractCommitment = (fileName: string): string => {
    const stem = fileName.replace(/\.json$/, "");
    const idx = stem.indexOf("-");
    if (idx >= 0) {
      return stem.substring(idx + 1);
    }
    return stem;
  };

  const buildClaimProof = (): ClaimBurnProof | null => {
    if (formState.proofSource === "file") {
      if (!formState.selectedFile) return null;
      return { FromFile: { file_name: formState.selectedFile } };
    } else {
      if (!formState.pastedProof) return null;
      try {
        const parsed = JSON.parse(formState.pastedProof);
        return { Contents: parsed };
      } catch {
        return null;
      }
    }
  };

  const isProofValid = (): boolean => {
    if (formState.proofSource === "file") {
      return formState.selectedFile !== "";
    }
    if (!formState.pastedProof) return false;
    try {
      JSON.parse(formState.pastedProof);
      return true;
    } catch {
      return false;
    }
  };

  const hasProof = isProofValid();
  const parsedFee = formState.fee !== "" ? Number(formState.fee) : NaN;
  const hasFee = !isNaN(parsedFee) && parsedFee > 0;
  const canEstimate = formState.account !== "" && hasProof;
  const canSubmit = canEstimate && hasFee;

  const onAccountChange = (e: SelectChangeEvent) => {
    setFormState({ ...formState, account: e.target.value, selectedFile: "" });
    setProofDetails(null);
  };

  const onTabChange = (_: React.SyntheticEvent, newValue: ProofSource) => {
    setFormState({ ...formState, proofSource: newValue });
    setProofError(null);
    setProofDetails(null);
  };

  const onFileSelect = async (e: SelectChangeEvent) => {
    const fileName = e.target.value;
    setFormState({ ...formState, selectedFile: fileName });
    setProofDetails(null);

    if (!fileName) return;

    setIsLoadingDetails(true);
    try {
      const resp = await burnProofsGet({ file_name: fileName });
      setProofDetails(resp.proof);
    } catch (err: any) {
      console.warn("Failed to load burn proof details:", err.message);
    } finally {
      setIsLoadingDetails(false);
    }
  };

  const onPastedProofChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    setFormState({ ...formState, pastedProof: value });
    if (value) {
      try {
        JSON.parse(value);
        setProofError(null);
      } catch {
        setProofError("Invalid JSON");
      }
    } else {
      setProofError(null);
    }
  };

  const onFeeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setFormState({ ...formState, fee: e.target.value });
  };

  const estimateFee = async () => {
    const proof = buildClaimProof();
    if (!proof) return;

    setIsEstimating(true);
    setFormState((prev) => ({ ...prev, disabled: true }));
    try {
      const resp = await accountsClaimBurn({
        account: { ComponentAddress: formState.account },
        claim_proof: proof,
        max_fee: 3000,
        is_dry_run: true,
      });

      const dryRunResult = resp.dry_run_result;
      if (!dryRunResult) {
        throw new Error("No dry run result returned");
      }

      const txResult = dryRunResult.finalize.result;
      if ("Reject" in txResult) {
        throw new Error(rejectReasonToString(txResult.Reject));
      }
      if ("AcceptFeeRejectRest" in txResult) {
        throw new Error(rejectReasonToString(txResult.AcceptFeeRejectRest[1]));
      }

      const feeReceipt = dryRunResult.finalize.fee_receipt;
      const fee = feeReceipt.total_fees_paid - feeReceipt.total_fee_overcharge;
      setFormState((prev) => ({ ...prev, fee: String(fee), disabled: false }));
    } catch (e: any) {
      setFormState((prev) => ({ ...prev, disabled: false }));
      setPopup({ title: "Fee estimation failed", error: true, message: e.message });
    } finally {
      setIsEstimating(false);
    }
  };

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();

    if (!hasFee) {
      await estimateFee();
      return;
    }

    const proof = buildClaimProof();
    if (!proof) return;

    setFormState((prev) => ({ ...prev, disabled: true }));
    try {
      const resp = await accountsClaimBurn({
        account: { ComponentAddress: formState.account },
        claim_proof: proof,
        max_fee: parsedFee,
        is_dry_run: false,
      });
      const waitResp = await transactionsWaitResult({ transaction_id: resp.transaction_id, timeout_secs: 30 });
      if (waitResp.status !== "Accepted") {
        throw new Error(`Transaction not accepted: ${waitResp.status}`);
      }
      setOpen(false);
      setPopup({ title: "Claimed", error: false });
      setFormState(INITIAL_FORM_STATE);
    } catch (e: any) {
      setFormState((prev) => ({ ...prev, disabled: false }));
      setPopup({ title: "Claim burn failed: " + e.message, error: true });
    }
  };

  const handleClickOpen = () => {
    setFormState(INITIAL_FORM_STATE);
    setProofError(null);
    setProofDetails(null);
    setOpen(true);
  };

  const handleClose = () => {
    setOpen(false);
  };

  return (
    <div>
      <Button variant="outlined" onClick={handleClickOpen}>
        Claim Burn
      </Button>
      <Dialog open={open} onClose={handleClose} maxWidth="sm" fullWidth>
        <PopupTitle onClose={handleClose} title="Claim Burn" />
        <DialogContent className="dialog-content">
          <Form onSubmit={onSubmit} className="flex-container-vertical" style={{ paddingTop: theme.spacing(1) }}>
            <FormControl>
              <InputLabel id="account-label">Account</InputLabel>
              <Select
                labelId="account-label"
                name="account"
                label="Account"
                value={formState.account}
                onChange={onAccountChange}
                style={{ flexGrow: 1, minWidth: "200px" }}
                disabled={formState.disabled}
              >
                {accountsList?.accounts?.map((account: AccountInfo, i: number) => (
                  <MenuItem key={i} value={account.account.component_address}>
                    <div>
                      <i>{account.account.name}</i>
                    </div>
                  </MenuItem>
                ))}
              </Select>
            </FormControl>

            <Tabs value={formState.proofSource} onChange={onTabChange} variant="fullWidth">
              <Tab label="From Server File" value="file" disabled={formState.disabled} />
              <Tab label="Paste Proof" value="paste" disabled={formState.disabled} />
            </Tabs>

            {formState.proofSource === "file" ? (
              <Box>
                {isLoadingProofs ? (
                  <Box display="flex" alignItems="center" gap={1} py={1}>
                    <CircularProgress size={20} />
                    <Typography variant="body2">Loading burn proof files...</Typography>
                  </Box>
                ) : burnProofFiles.length === 0 ? (
                  <Alert severity="info">No burn proofs found.</Alert>
                ) : (
                  <FormControl fullWidth>
                    <InputLabel id="proof-file-label">Burn Proof</InputLabel>
                    <Select
                      labelId="proof-file-label"
                      label="Burn Proof"
                      value={formState.selectedFile}
                      onChange={onFileSelect}
                      disabled={formState.disabled}
                    >
                      {burnProofFiles.map((proof) => (
                        <MenuItem key={proof.file_name} value={proof.file_name}>
                          {extractCommitment(proof.file_name)}
                          {proof.value !== null ? ` — ${formatCurrency(BigInt(proof.value), XTR_CURRENCY)}` : ""}
                        </MenuItem>
                      ))}
                    </Select>
                  </FormControl>
                )}
              </Box>
            ) : (
              <TextField
                name="pastedProof"
                label="Claim Proof (JSON)"
                value={formState.pastedProof}
                onChange={onPastedProofChange}
                multiline
                minRows={4}
                maxRows={10}
                style={{ flexGrow: 1 }}
                disabled={formState.disabled}
                error={proofError !== null}
                helperText={proofError}
              />
            )}

            {isLoadingDetails && (
              <Box display="flex" alignItems="center" gap={1} py={1}>
                <CircularProgress size={16} />
                <Typography variant="body2">Loading proof details...</Typography>
              </Box>
            )}

            {proofDetails && !isLoadingDetails && (
              <Box
                sx={{
                  border: 1,
                  borderColor: "divider",
                  borderRadius: 1,
                  p: 1.5,
                  display: "flex",
                  flexDirection: "column",
                  gap: 0.5,
                }}
              >
                <Typography variant="subtitle2">Burn Proof Summary</Typography>
                <Typography variant="body2">
                  <strong>Amount:</strong> {formatCurrency(BigInt(proofDetails.claim_proof.value), XTR_CURRENCY)}
                </Typography>
                <Typography variant="body2">
                  <strong>Commitment:</strong>{" "}
                  <span style={{ fontFamily: "monospace" }}>{truncateHex(proofDetails.claim_proof.commitment)}</span>
                </Typography>
                <Typography variant="body2">
                  <strong>L1 Block:</strong>{" "}
                  <span style={{ fontFamily: "monospace" }}>
                    {truncateHex(proofDetails.claim_proof.encoded_merkle_proof.block_hash)}
                  </span>
                </Typography>
                {selectedAccount &&
                  (proofDetails.claim_proof.burn_public_key === selectedAccount.account.owner_public_key ? (
                    <Alert severity="success" sx={{ mt: 0.5 }}>
                      Account matches burn proof
                    </Alert>
                  ) : (
                    <Alert severity="warning" sx={{ mt: 0.5 }}>
                      This burn proof was created for a different account
                    </Alert>
                  ))}
              </Box>
            )}

            <TextField
              name="fee"
              label="Max Transaction Fee"
              placeholder="Press Estimate Fee to calculate"
              value={formState.fee}
              onChange={onFeeChange}
              style={{ flexGrow: 1 }}
              disabled={formState.disabled}
            />

            <Box
              className="flex-container"
              style={{
                justifyContent: "flex-end",
              }}
            >
              <Button variant="outlined" onClick={handleClose} disabled={formState.disabled}>
                Cancel
              </Button>
              <Button
                variant="contained"
                type="submit"
                disabled={!canEstimate || formState.disabled || isEstimating}
                startIcon={isEstimating ? <CircularProgress size={16} /> : undefined}
              >
                {isEstimating ? "Estimating..." : hasFee ? "Claim Burn" : "Estimate Fee"}
              </Button>
            </Box>
          </Form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
