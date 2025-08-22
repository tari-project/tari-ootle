//  Copyright 2025. The Tari Project
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

import { FormEvent, useEffect, useState } from "react";
import { Form } from "react-router-dom";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import Box from "@mui/material/Box";
import { useTheme } from "@mui/material/styles";
import {
  Stepper,
  Step,
  StepLabel,
  Typography,
  Card,
  CardContent,
  Stack,
  Divider,
  CircularProgress,
} from "@mui/material";
import useAccountStore from "../../../store/accountStore";
import type {
  Account,
  ComponentAddressOrName,
  ResourceAddress,
  NonFungibleId,
  NonFungibleToken,
} from "@tari-project/typescript-bindings";
import Select from "@mui/material/Select";
import MenuItem from "@mui/material/MenuItem";
import Checkbox from "@mui/material/Checkbox";
import ListItemText from "@mui/material/ListItemText";
import { InputLabel } from "@mui/material";
import { SelectChangeEvent } from "@mui/material/Select/Select";
import { useListNfts, useNftsTransfer } from "../../../api/hooks/useNfts";
import { substateIdToString, formatXTM } from "../../../utils/helpers";
import { useAccountsList } from "../../../api/hooks/useAccounts";
import CopyAddress from "../../../Components/CopyAddress";

interface TransferNftProps {
  account?: Account;
  nftId: NonFungibleId;
  resourceAddress?: ResourceAddress;
}

export default function SendNft({ nftId, resourceAddress }: TransferNftProps) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button variant="outlined" onClick={() => setOpen(true)}>
        Send
      </Button>
      <TransferNftDialog
        open={open}
        handleClose={() => setOpen(false)}
        onSendComplete={() => setOpen(false)}
        preSelectedNftId={nftId}
        preSelectedResourceAddress={resourceAddress}
      />
    </>
  );
}

export interface TransferNftDialogProps {
  open: boolean;
  onSendComplete?: () => void;
  handleClose: () => void;
  preSelectedNftId?: NonFungibleId;
  preSelectedResourceAddress?: ResourceAddress;
}

interface NftListItem {
  address: string;
  name: string;
}

function getNftIdTypeAsName(nftId: NonFungibleId): string {
  const key = Object.keys(nftId)[0];
  switch (key) {
    case "U256":
      return "uuid";
    case "String":
      return "str";
    case "Uint32":
      return "u32";
    case "Uint64":
      return "u64";
    default:
      return "";
  }
}

function nftIdToString(nftId: NonFungibleId): string {
  const key = Object.keys(nftId)[0];
  // @ts-ignore
  const id = nftId[key].toString();
  return getNftIdTypeAsName(nftId) + "_" + id;
}

function getAccountSelector(account: Account): ComponentAddressOrName {
  return account.name
    ? {
        Name: account.name,
      }
    : {
        ComponentAddress: substateIdToString(account.address),
      };
}

type DialogStep = "form" | "confirmation" | "result";

export function TransferNftDialog(props: TransferNftDialogProps) {
  const { preSelectedNftId, preSelectedResourceAddress } = props;

  const INITIAL_VALUES = {
    payerAccount: "",
    nfts: preSelectedNftId ? [preSelectedNftId] : ([] as NonFungibleId[]),
    targetAccountPublicKey: "",
    maxFee: "",
    resourceAddress: (preSelectedResourceAddress || "") as ResourceAddress,
  };

  const [currentStep, setCurrentStep] = useState<DialogStep>("form");
  const [disabled, setDisabled] = useState(false);
  const [estimatedFee, setEstimatedFee] = useState<number | null>(null);
  const [isEstimatingFee, setIsEstimatingFee] = useState(false);
  const [transferResult, setTransferResult] = useState<{ success: boolean; message: string } | null>(null);
  const [transferFormState, setTransferFormState] = useState(INITIAL_VALUES);
  const [autoCloseCountdown, setAutoCloseCountdown] = useState<number | null>(null);
  const [autoCloseTimeoutId, setAutoCloseTimeoutId] = useState<NodeJS.Timeout | null>(null);
  const [validity, setValidity] = useState<object>({
    payerAccount: true,
    nfts: preSelectedNftId ? true : false,
    targetAccountPublicKey: false,
  });
  const [allValid, setAllValid] = useState(false);

  const { account, setPopup } = useAccountStore();
  if (!account) {
    return <></>;
  }

  //payer account
  const currentAccountSelector = getAccountSelector(account);
  const [payerAccount, setPayerAccount] = useState(currentAccountSelector);
  useEffect(() => {
    if (transferFormState.payerAccount != "") {
      setPayerAccount({
        ComponentAddress: transferFormState.payerAccount,
      });
    }
  }, [transferFormState.payerAccount]);

  // list NFTs
  const { data: loadedNfts, refetch: refetchNfts } = useListNfts({
    account: getAccountSelector(account),
  });

  // list all accounts for payer account selection
  let { data: accountsResp } = useAccountsList(0, 1000);
  let accounts = accountsResp?.accounts;

  refetchNfts().catch(console.error);

  const theme = useTheme();

  const { mutateAsync: calculateFeeEstimate } = useNftsTransfer({
    dry_run: true,
    max_fee: 3000,
    nfts: transferFormState.nfts,
    source_account: getAccountSelector(account),
    target_account_public_key: transferFormState.targetAccountPublicKey,
    fee_payer_account: payerAccount,
    resource_address: transferFormState.resourceAddress,
  });

  const { mutateAsync: sendTransferNftsTx } = useNftsTransfer({
    nfts: transferFormState.nfts,
    source_account: getAccountSelector(account),
    target_account_public_key: transferFormState.targetAccountPublicKey,
    dry_run: false,
    max_fee: parseInt(transferFormState.maxFee),
    fee_payer_account: payerAccount,
    resource_address: transferFormState.resourceAddress,
  });

  function setFormValue(e: React.ChangeEvent<HTMLInputElement>) {
    const { name, value } = e.target;

    setTransferFormState({
      ...transferFormState,
      [name]: value,
    });

    if (validity[name as keyof object] !== undefined) {
      setValidity({
        ...validity,
        [name]: e.target.validity.valid,
      });
    }

    // Auto-estimate fee when target account is entered and valid
    if (name === "targetAccountPublicKey" && value.trim() && value.match(/^[0-9a-fA-F]+$/)) {
      // Small delay to let state update, then estimate fee
      setTimeout(() => {
        estimateFeeWithTargetAccount(value).catch(() => {
          // Fee estimation failed, but don't block the user
        });
      }, 500);
    }
  }

  const estimateFeeWithTargetAccount = async (targetAccount: string) => {
    if (!account || isEstimatingFee || !targetAccount.trim()) {
      console.log(
        "Skipping fee estimation - account:",
        !!account,
        "isEstimating:",
        isEstimatingFee,
        "targetAccount:",
        !!targetAccount.trim(),
      );
      return;
    }

    console.log("Starting fee estimation with target account:", targetAccount);

    setIsEstimatingFee(true);

    // Ensure the form state is updated for fee estimation
    setTransferFormState((prev) => ({ ...prev, targetAccountPublicKey: targetAccount }));

    try {
      // Use a small delay to ensure state is updated
      await new Promise((resolve) => setTimeout(resolve, 100));

      const result = await calculateFeeEstimate?.();
      console.log("Fee estimation result:", result);

      if (result && "Accept" in result.result.result) {
        const fee = result.fee + 100; // Add buffer as per original comment
        console.log("Estimated fee:", fee);
        setEstimatedFee(fee);
        setTransferFormState((prev) => ({ ...prev, maxFee: fee.toString() }));
        return fee;
      } else {
        console.error("Fee estimation rejected:", result);
        throw new Error("Could not estimate transfer fee");
      }
    } catch (e: any) {
      console.error("Fee estimation error:", e);
      // Don't show popup for auto-estimation failures
      throw e;
    } finally {
      setIsEstimatingFee(false);
    }
  };

  const estimateFee = async () => {
    return estimateFeeWithTargetAccount(transferFormState.targetAccountPublicKey);
  };

  const handleFormSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!account) return;

    console.log("Form submitted, current fee:", transferFormState.maxFee);
    console.log("AllValid:", allValid);
    console.log("Validity state:", validity);
    console.log("Target account:", transferFormState.targetAccountPublicKey);

    // Check if target account is filled (minimum requirement)
    if (!transferFormState.targetAccountPublicKey.trim()) {
      setPopup({ title: "Missing information", error: true, message: "Please enter the target account public key" });
      return;
    }

    // Always estimate fee for better UX
    try {
      await estimateFee();
      setCurrentStep("confirmation");
    } catch (error) {
      console.error("Fee estimation failed:", error);
      // Don't proceed if fee estimation failed
    }
  };

  const onTransfer = async () => {
    if (!account) {
      return;
    }

    setDisabled(true);
    setCurrentStep("result");

    try {
      const result = await sendTransferNftsTx?.();
      if (result && "Accept" in result.result.result) {
        setTransferResult({
          success: true,
          message: "Your NFT has been successfully transferred!",
        });

        // Start countdown from 10 seconds
        setAutoCloseCountdown(10);

        // Create countdown interval
        const countdownInterval = setInterval(() => {
          setAutoCloseCountdown((prev) => {
            if (prev === null || prev <= 1) {
              clearInterval(countdownInterval);
              props.onSendComplete?.();
              handleClose();
              return null;
            }
            return prev - 1;
          });
        }, 1000);

        // Store the timeout ID for cleanup
        const timeoutId = setTimeout(() => {
          clearInterval(countdownInterval);
          props.onSendComplete?.();
          handleClose();
        }, 10000);
        setAutoCloseTimeoutId(timeoutId);
      } else {
        setTransferResult({
          success: false,
          message: "Transfer failed: Transaction was rejected",
        });
      }
    } catch (e: any) {
      setTransferResult({
        success: false,
        message: `Transfer failed: ${e.message}`,
      });
    } finally {
      setDisabled(false);
    }
  };

  const handleClose = () => {
    // Clear any active timeout
    if (autoCloseTimeoutId) {
      clearTimeout(autoCloseTimeoutId);
      setAutoCloseTimeoutId(null);
    }

    // Reset all states
    const freshInitialValues = {
      payerAccount: "",
      nfts: preSelectedNftId ? [preSelectedNftId] : ([] as NonFungibleId[]),
      targetAccountPublicKey: "",
      maxFee: "",
      resourceAddress: (preSelectedResourceAddress || "") as ResourceAddress,
    };
    setTransferFormState(freshInitialValues);
    setCurrentStep("form");
    setDisabled(false);
    setEstimatedFee(null);
    setIsEstimatingFee(false);
    setTransferResult(null);
    setAutoCloseCountdown(null);
    props.handleClose?.();
  };

  useEffect(() => {
    setAllValid(Object.values(validity).every((v) => v));
  }, [validity]);

  const [availableNfts, setAvailableNfts] = useState<NonFungibleToken[]>([]);
  useEffect(() => {
    if (loadedNfts !== undefined) {
      setAvailableNfts(loadedNfts);
    }
  }, [loadedNfts]);

  useEffect(() => {
    if (props.open) {
      // When dialog opens, ensure we have fresh state with correct NFT
      const freshInitialValues = {
        payerAccount: substateIdToString(account.address),
        nfts: preSelectedNftId ? [preSelectedNftId] : ([] as NonFungibleId[]),
        targetAccountPublicKey: "",
        maxFee: "",
        resourceAddress: (preSelectedResourceAddress || "") as ResourceAddress,
      };
      setTransferFormState(freshInitialValues);
    }
  }, [props.open, preSelectedNftId, preSelectedResourceAddress, account.address]);

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => {
      if (autoCloseTimeoutId) {
        clearTimeout(autoCloseTimeoutId);
      }
    };
  }, [autoCloseTimeoutId]);

  const handleNftsChange = (event: SelectChangeEvent<string[]>) => {
    if (typeof event.target.value == "string") {
      return;
    }
    const nftNamesSelected = event.target.value.map((s) => JSON.parse(s)) as NonFungibleId[];
    const nftsSelected = nftNamesSelected
      .map((nftId) => {
        return availableNfts.find((nft) => {
          return nftIdToString(nft.nft_id) === nftIdToString(nftId);
        })!;
      })
      .filter((value) => Boolean(value));

    setValidity({
      ...validity,
      nfts: nftsSelected.length > 0,
    });
    if (nftsSelected.length === 0) {
      return;
    }

    setTransferFormState({
      ...transferFormState,
      nfts: nftsSelected.map((item) => item.nft_id),
      // TODO: for simplicity, this dialog should transfer NFTS from a specific vault/resource -
      //       not for arbitrary NFTs from various vaults (this is not supported by the backend due to needlessly complexity/performance issues)
      resourceAddress: nftsSelected[0].resource_address,
    });
  };

  const handlePayerAccountChange = (event: SelectChangeEvent<string[]>) => {
    if (typeof event.target.value != "string") {
      return;
    }
    const payerAccountSelected = {
      ComponentAddress: event.target.value,
    };

    if (payerAccountSelected.ComponentAddress != "") {
      setValidity({
        ...validity,
        payerAccount: true,
      });

      setTransferFormState({
        ...transferFormState,
        payerAccount: event.target.value,
      });
    }
  };

  const steps = ["Enter Details", "Confirm Transfer", "Complete"];

  const getStepIndex = () => {
    switch (currentStep) {
      case "form":
        return 0;
      case "confirmation":
        return 1;
      case "result":
        return 2;
      default:
        return 0;
    }
  };

  return (
    <Dialog open={props.open} onClose={handleClose} maxWidth="md" fullWidth>
      <DialogTitle>
        Transfer NFT
        <Stepper activeStep={getStepIndex()} sx={{ mt: 2 }}>
          {steps.map((label) => (
            <Step key={label}>
              <StepLabel>{label}</StepLabel>
            </Step>
          ))}
        </Stepper>
      </DialogTitle>
      <DialogContent className="dialog-content">
        {currentStep === "form" && (
          <Form
            onSubmit={handleFormSubmit}
            className="flex-container-vertical"
            style={{ paddingTop: theme.spacing(1) }}
          >
            {accounts && (
              <>
                <InputLabel id="select-payer-account">Account (to pay fees)</InputLabel>
                <Select
                  id="select-payer-account"
                  name="payerAccount"
                  disabled={disabled}
                  displayEmpty
                  // @ts-ignore
                  value={
                    transferFormState.payerAccount ||
                    substateIdToString(accounts.find((a) => a.account.is_default)?.account.address) ||
                    ""
                  }
                  onChange={handlePayerAccountChange}
                  variant="outlined"
                >
                  {accounts.map((account) => (
                    <MenuItem key={account.account.name} value={substateIdToString(account.account.address)}>
                      {account.account.name} {account.account.is_default ? "(default)" : ""}
                    </MenuItem>
                  ))}
                </Select>
              </>
            )}
            <TextField
              name="targetAccountPublicKey"
              label="Target Account Public Key"
              value={transferFormState.targetAccountPublicKey}
              inputProps={{ pattern: "^[0-9a-fA-F]*$" }}
              required
              onChange={setFormValue}
              style={{ flexGrow: 1 }}
              disabled={disabled}
            />
            <TextField
              name="maxFee"
              label="Transaction Fee"
              value={
                isEstimatingFee
                  ? "Estimating..."
                  : transferFormState.maxFee
                    ? formatXTM(parseInt(transferFormState.maxFee))
                    : "Will be calculated automatically"
              }
              placeholder="Fee will be estimated automatically"
              disabled={true}
              style={{ flexGrow: 1 }}
            />

            {!preSelectedNftId ? (
              <>
                <InputLabel id="nft-select-label">Select NFT(s)</InputLabel>
                <Select
                  labelId="nft-select-label"
                  name="nfts"
                  id="nft-select"
                  multiple
                  value={transferFormState.nfts.map((nft) => JSON.stringify(nft))}
                  required
                  disabled={disabled}
                  onChange={handleNftsChange}
                  renderValue={(selected) => selected.map((item) => item).join(", ")}
                >
                  {availableNfts.map((nft, index) => (
                    <MenuItem key={index} value={JSON.stringify(nft.nft_id)}>
                      <Checkbox
                        checked={transferFormState.nfts.some((id) => nftIdToString(id) == nftIdToString(nft.nft_id))}
                      />
                      <ListItemText primary={nftIdToString(nft.nft_id)} />
                    </MenuItem>
                  ))}
                </Select>
              </>
            ) : (
              <TextField
                label="Selected NFT"
                value={nftIdToString(preSelectedNftId)}
                disabled
                variant="outlined"
                style={{ flexGrow: 1 }}
              />
            )}

            <Box
              className="flex-container"
              style={{
                justifyContent: "flex-end",
              }}
            >
              <Button variant="outlined" onClick={handleClose} disabled={disabled}>
                Cancel
              </Button>
              <Button
                variant="contained"
                type="submit"
                disabled={disabled || !transferFormState.targetAccountPublicKey.trim()}
              >
                Continue
              </Button>
            </Box>
          </Form>
        )}

        {currentStep === "confirmation" && (
          <Stack spacing={3} sx={{ py: 2 }}>
            <Typography variant="h5">You are about to:</Typography>

            <Card variant="outlined">
              <CardContent>
                <Stack spacing={2}>
                  <Box>
                    <Typography variant="h6" gutterBottom>
                      NFT Transfer
                    </Typography>
                    <Divider />
                  </Box>

                  <Box>
                    <Typography variant="subtitle2" color="text.secondary">
                      NFT:
                    </Typography>
                    <Typography>{preSelectedNftId ? nftIdToString(preSelectedNftId) : "Multiple NFTs"}</Typography>
                  </Box>

                  <Box>
                    <Typography variant="subtitle2" color="text.secondary">
                      To Account:
                    </Typography>
                    <Typography variant="subtitle1">
                      {/* {transferFormState.targetAccountPublicKey} */}
                      <CopyAddress address={transferFormState.targetAccountPublicKey} />
                    </Typography>
                  </Box>

                  <Box>
                    <Typography variant="subtitle2" color="text.secondary">
                      Transaction Fee:
                    </Typography>
                    <Typography>{formatXTM(parseInt(transferFormState.maxFee))}</Typography>
                  </Box>

                  <Box>
                    <Typography variant="subtitle2" color="text.secondary">
                      Fee paid by:
                    </Typography>
                    <Typography>
                      {accounts?.find(
                        (acc) => substateIdToString(acc.account.address) === transferFormState.payerAccount,
                      )?.account.name || transferFormState.payerAccount}
                    </Typography>
                    <Typography variant="subtitle1">
                      <CopyAddress address={transferFormState.targetAccountPublicKey} />
                    </Typography>
                  </Box>
                </Stack>
              </CardContent>
            </Card>

            <Stack direction="row" justifyContent="space-between" sx={{ mt: 3 }}>
              <Button variant="outlined" onClick={() => setCurrentStep("form")}>
                Back
              </Button>
              <Button variant="contained" onClick={onTransfer} disabled={disabled}>
                Confirm and Send
              </Button>
            </Stack>
          </Stack>
        )}

        {currentStep === "result" && (
          <Stack spacing={3} sx={{ py: 2, textAlign: "center" }}>
            {disabled ? (
              <>
                <CircularProgress size={60} />
                <Typography variant="h6">Sending NFT...</Typography>
                <Typography color="text.secondary">Please wait while your transaction is processed.</Typography>
              </>
            ) : transferResult ? (
              <>
                <Typography variant="h5" color={transferResult.success ? "success.main" : "error.main"}>
                  {transferResult.success ? "✅ Transfer Successful!" : "❌ Transfer Failed"}
                </Typography>
                <Typography>{transferResult.message}</Typography>
                {transferResult.success && autoCloseCountdown && (
                  <Typography variant="body2" color="text.secondary">
                    This dialog will close automatically in {autoCloseCountdown} seconds
                  </Typography>
                )}
                <Button variant="contained" onClick={handleClose}>
                  {transferResult.success && autoCloseCountdown ? `Close Now` : "Close"}
                </Button>
              </>
            ) : null}
          </Stack>
        )}
      </DialogContent>
    </Dialog>
  );
}

function unionGet<T extends object>(object: T, key: keyof T): T[keyof T] | null {
  return key in object ? object[key] : null;
}
