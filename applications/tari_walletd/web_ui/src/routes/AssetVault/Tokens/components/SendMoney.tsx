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
import { useAccountsGetBalances, useAccountsTransfer } from "@api/hooks/useAccounts";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import { SelectChangeEvent } from "@mui/material/Select";
import useAccountStore from "@store/accountStore";
import {
  BadgeUsage,
  BalanceEntry,
  rejectReasonToString,
  ResourceAddress,
  ResourceType,
  substateIdToString,
  SwapPoolInfo,
  TARI_TOKEN,
  UtxoInputSelection,
} from "@tari-project/ootle-ts-bindings";
import { parseAmountToBaseUnits } from "@utils/helpers";
import { swapPoolGetExchangeRate, swapPoolsList, transactionsWaitResult } from "@utils/json_rpc";
import { FormEvent, useCallback, useEffect, useState } from "react";
import ConfirmationStep from "../steps/ConfirmationStep";
import FormStep, { FormError, PoolRateInfo, SendMoneyFormState } from "../steps/FormStep";
import ResultStep, { TransferResult } from "../steps/ResultStep";

export interface SendMoneyDialogProps {
  open: boolean;
  resource_address?: ResourceAddress;
  resource_type: ResourceType;
  onSendComplete?: () => void;
  handleClose: () => void;
  token_symbol: string;
}
const U64_MAX = 2n ** 64n - 1n;

export function SendMoneyDialog(props: SendMoneyDialogProps) {
  const INITIAL_VALUES: SendMoneyFormState = {
    address: "",
    outputToRevealed: false,
    inputSelection: "PreferRevealed",
    amount: "",
    fee: "",
    badge: null,
    memo: "",
    attachSenderAddress: false,
    swapPoolAddress: "",
    swapInputAmount: "",
  };

  const [activeStep, setActiveStep] = useState(0);
  const [useBadge, setUseBadge] = useState(false);
  const [disabled, setDisabled] = useState(false);
  const [isEstimatingFee, setIsEstimatingFee] = useState(false);
  const [transferFormState, setTransferFormState] = useState(INITIAL_VALUES);
  const [transferResult, setTransferResult] = useState<TransferResult | undefined>();
  const [formError, setFormError] = useState<FormError | null>(null);
  const { mutateAsync: sendIt } = useAccountsTransfer();

  // Pool state
  const [knownPools, setKnownPools] = useState<SwapPoolInfo[]>([]);
  const [isLoadingPools, setIsLoadingPools] = useState(false);
  const [poolRate, setPoolRate] = useState<PoolRateInfo | null>(null);
  const [poolError, setPoolError] = useState<string | null>(null);
  const [isLoadingPoolRate, setIsLoadingPoolRate] = useState(false);

  const { account } = useAccountStore();

  const { data } = useAccountsGetBalances(account?.component_address);

  // Fetch known pools when dialog opens for stealth non-TARI tokens
  useEffect(() => {
    if (props.open && props.resource_type === "Stealth" && props.resource_address !== TARI_TOKEN) {
      setIsLoadingPools(true);
      swapPoolsList({
        resource_pair: props.resource_address ? [TARI_TOKEN, props.resource_address] : null,
        limit: 10n,
        offset: 0n,
      })
        .then((resp) => {
          setKnownPools(resp.pools);
        })
        .catch((e) => {
          console.warn("Failed to fetch swap pools:", e);
          setKnownPools([]);
        })
        .finally(() => setIsLoadingPools(false));
    }
  }, [props.open, props.resource_type, props.resource_address]);

  const fetchPoolRate = useCallback(async (poolAddress: string) => {
    if (!poolAddress) {
      setPoolRate(null);
      setPoolError(null);
      return;
    }

    setIsLoadingPoolRate(true);
    setPoolError(null);
    try {
      const resp = await swapPoolGetExchangeRate({ pool_address: poolAddress, desired_tari_output: null });
      setPoolRate({
        resource_a: resp.resource_a,
        balance_a: BigInt(resp.balance_a),
        resource_b: resp.resource_b,
        balance_b: BigInt(resp.balance_b),
      });
      setPoolError(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setPoolError(
        msg.includes("not a liquidity pool")
          ? "This component is not a liquidity pool"
          : `Failed to fetch pool: ${msg}`,
      );
      setPoolRate(null);
    } finally {
      setIsLoadingPoolRate(false);
    }
  }, []);

  if (!account) {
    return null;
  }

  const badges = data?.balances
    ?.filter((b: BalanceEntry) => b.resource_type === "NonFungible" && BigInt(b.balance) > 0n)
    .map((b: BalanceEntry) => b.resource_address) as string[];

  // Find the available balance for the resource we're trying to send
  const balanceEntry = data?.balances?.find((b: BalanceEntry) => b.resource_address === props.resource_address);

  // Check if user has TARI balance available (for recommending direct fee payment)
  const tariBalanceEntry = data?.balances?.find((b: BalanceEntry) => b.resource_address === TARI_TOKEN);
  const hasTariBalance = tariBalanceEntry
    ? BigInt(tariBalanceEntry.balance) + BigInt(tariBalanceEntry.confidential_balance) >= 500n
    : false;

  if (!balanceEntry) {
    console.warn("No balance entry found for resource", props.resource_address);
    return null;
  }

  // Function to calculate available balance based on input selection
  const calculateAvailableBalance = () => {
    if (!balanceEntry) return undefined;

    const revealedBalance = BigInt(balanceEntry.balance);
    const confidentialBalance = BigInt(balanceEntry.confidential_balance);

    let result;
    switch (transferFormState.inputSelection) {
      case "RevealedOnly":
        result = revealedBalance;
        break;
      case "ConfidentialOnly":
        result = confidentialBalance;
        break;
      case "PreferRevealed":
      case "PreferConfidential":
        // For prefer options, show total available (revealed + confidential)
        result = revealedBalance + confidentialBalance;
        break;
      default:
        result = revealedBalance + confidentialBalance;
        break;
    }

    return result;
  };

  const availableBalance = calculateAvailableBalance();

  function setFormValue(name: string, value: string) {
    setFormError(null);

    // For amount fields, parse the input to allow decimal values
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

    setTransferFormState({
      ...transferFormState,
      [name]: processedValue,
    });
  }

  function setSelectFormValue(e: SelectChangeEvent<unknown>) {
    setFormError(null);
    setTransferFormState({
      ...transferFormState,
      [e.target.name]: e.target.value,
    });
  }

  function setCheckboxFormValue(e: React.ChangeEvent<HTMLInputElement>) {
    setFormError(null);
    setTransferFormState({
      ...transferFormState,
      [e.target.name]: e.target.checked,
    });
  }

  const handleUseBadgeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setFormError(null);
    setUseBadge(e.target.checked);
    if (!e.target.checked) {
      setTransferFormState({
        ...transferFormState,
        badge: null,
      });
    }
  };

  const handlePoolSelect = (poolAddress: string) => {
    setTransferFormState((prev) => ({
      ...prev,
      swapPoolAddress: poolAddress,
      swapInputAmount: "", // Reset calculated amount
    }));
    if (poolAddress) {
      fetchPoolRate(poolAddress);
    } else {
      setPoolRate(null);
      setPoolError(null);
    }
  };

  const estimateFee = async () => {
    if (!account || isEstimatingFee || !transferFormState.address.trim() || !transferFormState.amount) {
      return;
    }
    if (!balanceEntry) {
      console.warn("No balance entry found for resource", props.resource_address);
      return;
    }

    setIsEstimatingFee(true);

    try {
      const amount = parseAmountToBaseUnits(transferFormState.amount, balanceEntry.divisibility);
      if (!transferFormState.outputToRevealed && amount > U64_MAX) {
        throw new RangeError(
          `Amount exceeds maximum value for a UTXO (${U64_MAX / 10n ** BigInt(balanceEntry.divisibility)} ${props.token_symbol})`,
        );
      }

      // For dry-run with a swap pool, we need a preliminary swap input amount.
      // Ask the server to calculate one using a generous initial fee estimate.
      let dryRunSwapInputAmount: bigint | null = null;
      if (transferFormState.swapPoolAddress && poolRate) {
        const prelimResp = await swapPoolGetExchangeRate({
          pool_address: transferFormState.swapPoolAddress,
          desired_tari_output: 3000,
        });
        if (prelimResp.swap_input_amount != null) {
          dryRunSwapInputAmount = BigInt(prelimResp.swap_input_amount);
        }
      }

      // Create transfer object with current form state
      const currentTransfer = {
        account: substateIdToString(account.component_address),
        amount,
        resource_address: props.resource_address || TARI_TOKEN,
        destination_address: transferFormState.address,
        resourceType: props.resource_type,
        output_to_revealed: transferFormState.outputToRevealed,
        input_selection: transferFormState.inputSelection as UtxoInputSelection,
        badge_usage: transferFormState.badge ? { Resource: transferFormState.badge } : ("None" as BadgeUsage),
        output_memo:
          transferFormState.memo && !transferFormState.attachSenderAddress
            ? { Message: transferFormState.memo }
            : undefined,
        attach_sender_address: transferFormState.attachSenderAddress,
        swap_pool_address: transferFormState.swapPoolAddress || null,
        swap_input_amount: dryRunSwapInputAmount,
      };

      const result = await sendIt?.({ ...currentTransfer, dry_run: true, max_fee: 1 });
      const resp = await transactionsWaitResult({ transaction_id: result.transaction_id, timeout_secs: null });
      const transactionResult = resp.result?.result;

      if (!transactionResult) {
        throw new Error("Fee estimation failed");
      }
      if ("Reject" in transactionResult) {
        throw new Error(`Transaction rejected: ${rejectReasonToString(transactionResult.Reject)}`);
      }
      if ("AcceptFeeRejectRest" in transactionResult) {
        throw new Error(`Transaction rejected: ${rejectReasonToString(transactionResult.AcceptFeeRejectRest[1])}`);
      }

      const estimatedFee = resp.final_fee;

      // Ask the server to calculate the precise swap input from the actual fee estimate
      let swapInputAmount = "";
      if (transferFormState.swapPoolAddress) {
        const rateResp = await swapPoolGetExchangeRate({
          pool_address: transferFormState.swapPoolAddress,
          desired_tari_output: Number(estimatedFee),
        });
        if (rateResp.swap_input_amount != null) {
          swapInputAmount = rateResp.swap_input_amount.toString();
        }
        // Also refresh the pool rate display
        setPoolRate({
          resource_a: rateResp.resource_a,
          balance_a: BigInt(rateResp.balance_a),
          resource_b: rateResp.resource_b,
          balance_b: BigInt(rateResp.balance_b),
        });
      }

      setTransferFormState((prevState) => ({
        ...prevState,
        fee: estimatedFee.toString(),
        swapInputAmount,
      }));
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
    if (!transferFormState.address.trim() || !transferFormState.amount) {
      return;
    }

    // If a swap pool is selected, ensure we have a valid rate
    if (transferFormState.swapPoolAddress && !poolRate) {
      setFormError({
        type: "general",
        message: "Waiting for pool exchange rate. Please try again.",
      });
      return;
    }

    // If no fee is calculated yet, estimate it before proceeding
    if (!transferFormState.fee) {
      try {
        await estimateFee();
      } catch (error) {
        setFormError({
          type: "general",
          message: `Failed to estimate fee: ${error}`,
        });
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
      const amount = parseAmountToBaseUnits(transferFormState.amount, balanceEntry.divisibility);
      if (!transferFormState.outputToRevealed && amount > U64_MAX) {
        throw new RangeError("Amount exceeds maximum value for a UTXO");
      }
      const transfer = {
        account: substateIdToString(account.component_address),
        amount,
        resource_address: props.resource_address!,
        destination_address: transferFormState.address,
        resourceType: props.resource_type,
        output_to_revealed: transferFormState.outputToRevealed,
        input_selection: transferFormState.inputSelection as UtxoInputSelection,
        badge_usage: transferFormState.badge ? { Resource: transferFormState.badge } : ("None" as BadgeUsage),
        output_memo:
          transferFormState.memo && !transferFormState.attachSenderAddress
            ? { Message: transferFormState.memo }
            : undefined,
        attach_sender_address: transferFormState.attachSenderAddress,
        swap_pool_address: transferFormState.swapPoolAddress || null,
        swap_input_amount: transferFormState.swapInputAmount ? BigInt(transferFormState.swapInputAmount) : null,
      };

      const submitResult = await sendIt?.({
        ...transfer,
        dry_run: false,
        max_fee: parseInt(transferFormState.fee),
      });

      // Wait for the transaction to be finalized
      const waitResult = await transactionsWaitResult({
        transaction_id: submitResult.transaction_id,
        timeout_secs: 120,
      });

      const txResult = waitResult.result?.result;
      if (txResult && "Accept" in txResult) {
        setTransferResult({
          success: true,
          message: "Transfer completed successfully",
        });
      } else if (txResult && "AcceptFeeRejectRest" in txResult) {
        setTransferResult({
          success: false,
          message: `Transfer rejected: ${rejectReasonToString(txResult.AcceptFeeRejectRest[1])}`,
        });
      } else if (txResult && "Reject" in txResult) {
        setTransferResult({
          success: false,
          message: `Transfer rejected: ${rejectReasonToString(txResult.Reject)}`,
        });
      } else {
        setTransferResult({
          success: false,
          message: `Transaction status: ${waitResult.status}`,
        });
      }
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
    setTransferResult(undefined);
    setUseBadge(false);
    setDisabled(false);
    setPoolRate(null);
    setPoolError(null);
    props.handleClose?.();
    // Call onSendComplete only after successful transfer when dialog closes
    if (wasSuccessful) {
      setTransferFormState(INITIAL_VALUES);
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
            divisibility={balanceEntry.divisibility}
            formError={formError}
            knownPools={knownPools}
            isLoadingPools={isLoadingPools}
            poolRate={poolRate}
            poolError={poolError}
            isLoadingPoolRate={isLoadingPoolRate}
            onSubmit={handleFormSubmit}
            onCancel={handleClose}
            onFormValueChange={setFormValue}
            onSelectFormValueChange={setSelectFormValue}
            onCheckboxFormValueChange={setCheckboxFormValue}
            onUseBadgeChange={handleUseBadgeChange}
            onPoolSelect={handlePoolSelect}
            hasTariBalance={hasTariBalance}
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
            poolRate={poolRate}
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
      <PopupTitle onClose={handleClose} title="Send Funds" />
      <DialogContent>{renderStepContent()}</DialogContent>
    </Dialog>
  );
}
