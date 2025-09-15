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

import { FormEvent, useState, useEffect } from "react";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import IconButton from "@mui/material/IconButton";
import CloseIcon from "@mui/icons-material/Close";
import { useAccountsGetBalances, useAccountsTransfer } from "@api/hooks/useAccounts";
import useAccountStore from "@store/accountStore";
import { SelectChangeEvent } from "@mui/material/Select/Select";
import {
  BalanceEntry,
  ConfidentialTransferInputSelection,
  ResourceAddress,
  ResourceType,
  substateIdToString,
  XTR,
} from "@tari-project/typescript-bindings";
import { transactionsWaitResult } from "@utils/json_rpc";
import FormStep, { SendMoneyFormState } from "../steps/FormStep";
import ConfirmationStep from "../steps/ConfirmationStep";
import ResultStep, { TransferResult } from "../steps/ResultStep";
import { Divider, Stack, Typography } from "@mui/material";

export default function SendMoney() {
  const [open, setOpen] = useState(false);
  const { account } = useAccountStore();

  const { data } = useAccountsGetBalances(account ? substateIdToString(account.address) : "");
  const xtrBalanceEntry = data?.balances?.find((b) => b.resource_address === XTR);

  return (
    <>
      <Button variant="outlined" onClick={() => setOpen(true)}>
        Send Tari
      </Button>
      <SendMoneyDialog
        open={open}
        handleClose={() => setOpen(false)}
        onSendComplete={() => setOpen(false)}
        resource_type="Confidential"
        resource_address={XTR}
        token_symbol={xtrBalanceEntry?.token_symbol || ""}
      />
    </>
  );
}

export interface SendMoneyDialogProps {
  open: boolean;
  resource_address?: ResourceAddress;
  resource_type: ResourceType;
  onSendComplete?: () => void;
  handleClose: () => void;
  token_symbol: string;
}

export function SendMoneyDialog(props: SendMoneyDialogProps) {
  const INITIAL_VALUES: SendMoneyFormState = {
    publicKey: "",
    outputToConfidential: false,
    inputSelection: "PreferRevealed",
    amount: "",
    fee: "",
    badge: null,
  };

  const [activeStep, setActiveStep] = useState(0);
  const [useBadge, setUseBadge] = useState(false);
  const [disabled, setDisabled] = useState(false);
  const [isEstimatingFee, setIsEstimatingFee] = useState(false);
  const [transferFormState, setTransferFormState] = useState(INITIAL_VALUES);
  const [transferResult, setTransferResult] = useState<TransferResult | undefined>();
  const { mutateAsync: sendIt } = useAccountsTransfer();

  const { account, setPopup } = useAccountStore();

  if (!account) {
    return null;
  }

  const { data } = useAccountsGetBalances(substateIdToString(account.address));
  const badges = data?.balances
    ?.filter((b: BalanceEntry) => b.resource_type === "NonFungible" && BigInt(b.balance) > 0n)
    .map((b: BalanceEntry) => b.resource_address) as string[];

  // Find the available balance for the resource we're trying to send
  const balanceEntry = data?.balances?.find(
    (b: BalanceEntry) => b.resource_address === (props.resource_address || XTR),
  );

  // Function to calculate available balance based on input selection
  const calculateAvailableBalance = () => {
    if (!balanceEntry) return undefined;

    const revealedBalance = BigInt(balanceEntry.balance);
    const confidentialBalance = BigInt(balanceEntry.confidential_balance);
    const divisor = Math.pow(10, balanceEntry.divisibility);

    let result;
    switch (transferFormState.inputSelection) {
      case "RevealedOnly":
        result = Number(revealedBalance) / divisor;
        break;
      case "ConfidentialOnly":
        result = Number(confidentialBalance) / divisor;
        break;
      case "PreferRevealed":
      case "PreferConfidential":
        // For prefer options, show total available (revealed + confidential)
        result = Number(revealedBalance + confidentialBalance) / divisor;
        break;
      default:
        result = Number(revealedBalance + confidentialBalance) / divisor;
        break;
    }

    return result;
  };

  const availableBalance = calculateAvailableBalance();

  const transfer = {
    account: substateIdToString(account.address),
    amount: Math.floor((parseFloat(transferFormState.amount) || 0) * Math.pow(10, balanceEntry?.divisibility || 6)),
    resource_address: props.resource_address!,
    destination_public_key: transferFormState.publicKey,
    resourceType: props.resource_type,
    output_to_revealed: !transferFormState.outputToConfidential,
    input_selection: transferFormState.inputSelection as ConfidentialTransferInputSelection,
    badge: transferFormState.badge,
  };

  function setFormValue(e: React.ChangeEvent<HTMLInputElement>) {
    const { name, value } = e.target;

    // For amount field, parse the input to allow decimal values
    let processedValue = value;
    if (name === "amount" && value) {
      // Remove currency symbol and extra spaces, but keep numbers and decimal point
      processedValue = value.replace(/[^\d.]/g, "");
      // Ensure only one decimal point
      const parts = processedValue.split(".");
      if (parts.length > 2) {
        processedValue = parts[0] + "." + parts.slice(1).join("");
      }
    }

    // Clear fee when amount or publicKey changes to trigger re-estimation
    const shouldClearFee = (name === "amount" || name === "publicKey") && transferFormState.fee;

    setTransferFormState({
      ...transferFormState,
      [name]: processedValue,
      ...(shouldClearFee ? { fee: "" } : {}),
    });
  }

  function setSelectFormValue(e: SelectChangeEvent<unknown>) {
    setTransferFormState({
      ...transferFormState,
      [e.target.name]: e.target.value,
    });
  }

  function setCheckboxFormValue(e: React.ChangeEvent<HTMLInputElement>) {
    setTransferFormState({
      ...transferFormState,
      [e.target.name]: e.target.checked,
    });
  }

  const handleUseBadgeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setUseBadge(e.target.checked);
    if (!e.target.checked) {
      setTransferFormState({
        ...transferFormState,
        badge: null,
      });
    }
  };

  const estimateFee = async () => {
    if (!account || isEstimatingFee || !transferFormState.publicKey.trim() || !transferFormState.amount) {
      return;
    }

    setIsEstimatingFee(true);

    try {
      // Create transfer object with current form state
      const currentTransfer = {
        account: substateIdToString(account.address),
        amount: Math.floor((parseFloat(transferFormState.amount) || 0) * Math.pow(10, balanceEntry?.divisibility || 6)),
        resource_address: props.resource_address || XTR,
        destination_public_key: transferFormState.publicKey,
        resourceType: props.resource_type,
        output_to_revealed: !transferFormState.outputToConfidential,
        input_selection: transferFormState.inputSelection as ConfidentialTransferInputSelection,
        badge: transferFormState.badge,
      };

      const result = await sendIt?.({ ...currentTransfer, dry_run: true, max_fee: 3000 });
      const resp = await transactionsWaitResult({ transaction_id: result.transaction_id, timeout_secs: null });
      const transactionResult = resp.result?.result;

      if (!transactionResult || !("Accept" in transactionResult)) {
        throw new Error("Fee estimation failed");
      }

      const fee = resp.final_fee + 100;
      setTransferFormState((prevState) => ({ ...prevState, fee: fee.toString() }));
    } catch (error) {
      console.error("Fee estimation error:", error);
      // Don't block the user if fee estimation fails
    } finally {
      setIsEstimatingFee(false);
    }
  };

  const handleFormSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!account) {
      return;
    }

    // Check if required fields are filled
    if (!transferFormState.publicKey.trim() || !transferFormState.amount) {
      return;
    }

    // If no fee is calculated yet, estimate it before proceeding
    if (!transferFormState.fee) {
      try {
        await estimateFee();
      } catch (error) {
        console.error("Fee estimation failed:", error);
        return;
      }
    }

    setActiveStep(1);
  };

  const handleConfirm = async () => {
    if (!account) {
      return;
    }

    setDisabled(true);
    setActiveStep(2);

    try {
      await sendIt?.({
        ...transfer,
        dry_run: false,
        max_fee: parseInt(transferFormState.fee),
      });

      setTransferResult({
        success: true,
        message: "Transfer completed successfully",
      });
      // Auto-close after 10 seconds - don't call onSendComplete immediately
    } catch (error) {
      setTransferResult({
        success: false,
        message: error instanceof Error ? error.message : "Transfer failed",
      });
    } finally {
      setDisabled(false);
    }
  };

  const handleClose = () => {
    const wasSuccessful = transferResult?.success;
    setActiveStep(0);
    setTransferFormState(INITIAL_VALUES);
    setTransferResult(undefined);
    setUseBadge(false);
    setDisabled(false);
    props.handleClose?.();
    // Call onSendComplete only after successful transfer when dialog closes
    if (wasSuccessful) {
      props.onSendComplete?.();
    }
  };

  const handleBack = () => {
    setActiveStep(activeStep - 1);
  };

  const renderStepContent = () => {
    switch (activeStep) {
      case 0:
        return (
          <FormStep
            resource_address={props.resource_address}
            resource_type={props.resource_type}
            badges={badges}
            transferFormState={transferFormState}
            disabled={disabled}
            useBadge={useBadge}
            isEstimatingFee={isEstimatingFee}
            availableBalance={availableBalance}
            token_symbol={props.token_symbol}
            divisibility={balanceEntry?.divisibility || 6}
            onSubmit={handleFormSubmit}
            onCancel={handleClose}
            onFormValueChange={setFormValue}
            onSelectFormValueChange={setSelectFormValue}
            onCheckboxFormValueChange={setCheckboxFormValue}
            onUseBadgeChange={handleUseBadgeChange}
          />
        );
      case 1:
        return (
          <ConfirmationStep
            resource_address={props.resource_address}
            resource_type={props.resource_type}
            transferFormState={transferFormState}
            disabled={disabled}
            onBack={handleBack}
            onConfirm={handleConfirm}
            token_symbol={props.token_symbol}
            divisibility={balanceEntry?.divisibility || 6}
          />
        );
      case 2:
        return <ResultStep disabled={disabled} transferResult={transferResult} onClose={handleClose} />;
      default:
        return null;
    }
  };

  return (
    <Dialog open={props.open} onClose={handleClose} maxWidth="md" fullWidth>
      <DialogTitle sx={{ m: 0, p: 2, display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <Stack direction="row" alignItems="center" justifyContent="center" width="100%" spacing={2}>
          <Typography variant="h4">Send Tari</Typography>
        </Stack>
        <IconButton aria-label="close" onClick={handleClose}>
          <CloseIcon />
        </IconButton>
      </DialogTitle>
      <DialogContent>
        <Divider />
        {renderStepContent()}
      </DialogContent>
    </Dialog>
  );
}
