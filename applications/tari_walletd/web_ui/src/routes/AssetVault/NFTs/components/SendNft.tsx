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

import { FormEvent, useEffect, useState, useMemo } from "react";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import { SelectChangeEvent } from "@mui/material/Select/Select";
import useAccountStore from "@store/accountStore";
import type {
  Account,
  ComponentAddressOrName,
  ResourceAddress,
  NonFungibleId,
  NonFungibleToken,
} from "@tari-project/typescript-bindings";
import { useNftsTransfer, useNFTsList } from "@api/hooks/useNfts";
import { substateIdToString } from "@utils/helpers";
import { useAccountsList } from "@api/hooks/useAccounts";
import { useNftTransferStore } from "@store/nftTransferStore";
import FormStep from "../steps/FormStep";
import ConfirmationStep from "../steps/ConfirmationStep";
import ResultStep from "../steps/ResultStep";
import PopupTitle from "@/components/PopupTitle";

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

function getAccountSelector(account: Account): ComponentAddressOrName {
  return {
    ComponentAddress: account.component_address,
  };
}

function nftIdToString(nftId: NonFungibleId): string {
  const key = Object.keys(nftId)[0];
  // @ts-ignore
  const id = nftId[key].toString();
  const typeName = getNftIdTypeAsName(nftId);
  return typeName + "_" + id;
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

export function TransferNftDialog(props: TransferNftDialogProps) {
  const { preSelectedNftId, preSelectedResourceAddress } = props;
  const { account, setPopup } = useAccountStore();

  const {
    currentStep,
    transferFormState,
    setCurrentStep,
    setDisabled,
    setTransferFormState,
    setValidity,
    setIsEstimatingFee,
    setTransferResult,
    setAutoCloseTimeoutId,
    initializeFormState,
    resetState,
  } = useNftTransferStore();

  const [availableNfts, setAvailableNfts] = useState<NonFungibleToken[]>([]);
  const [isEstimatingFee, setLocalIsEstimatingFee] = useState(false);

  // Memoize account selectors to prevent infinite re-renders - now nullable
  const sourceAccount = useMemo(() => (account ? getAccountSelector(account) : null), [account]);
  const feePayerAccount = useMemo(() => {
    if (!sourceAccount) return null;
    return transferFormState.payerAccount ? { ComponentAddress: transferFormState.payerAccount } : sourceAccount;
  }, [transferFormState.payerAccount, sourceAccount]);

  // List NFTs - only enabled when account is present
  const { data: nftsResponse, refetch: refetchNfts } = useNFTsList(account?.component_address!, 0, 1000);
  const loadedNfts = nftsResponse?.nfts;

  // List all accounts for payer account selection - only enabled when account is present
  const { data: accountsResp } = useAccountsList(0, 1000, !!account);
  const accounts = accountsResp?.accounts;

  // Only refetch NFTs when account is available
  useEffect(() => {
    if (account) {
      refetchNfts().catch(console.error);
    }
  }, [account, refetchNfts]);

  // Memoize hook parameters to prevent re-renders
  const feeEstimateParams = useMemo(
    () => ({
      dry_run: true,
      max_fee: 3000,
      nfts: transferFormState.nfts,
      source_account: sourceAccount!,
      target_account_public_key: transferFormState.targetAccountAddress,
      fee_payer_account: feePayerAccount!,
      resource_address: transferFormState.resourceAddress,
    }),
    [
      transferFormState.nfts,
      sourceAccount,
      transferFormState.targetAccountAddress,
      feePayerAccount,
      transferFormState.resourceAddress,
    ],
  );

  const transferParams = useMemo(
    () => ({
      nfts: transferFormState.nfts,
      source_account: sourceAccount!,
      target_account_public_key: transferFormState.targetAccountAddress,
      dry_run: false,
      max_fee: parseInt(transferFormState.maxFee) || 3000,
      fee_payer_account: feePayerAccount!,
      resource_address: transferFormState.resourceAddress,
    }),
    [
      transferFormState.nfts,
      sourceAccount,
      transferFormState.targetAccountAddress,
      transferFormState.maxFee,
      feePayerAccount,
      transferFormState.resourceAddress,
    ],
  );

  // Fee estimation and transfer hooks
  const { mutateAsync: calculateFeeEstimate } = useNftsTransfer(feeEstimateParams);
  const { mutateAsync: sendTransferNftsTx } = useNftsTransfer(transferParams);

  const estimateFeeWithTargetAccount = async (targetAccount: string) => {
    if (!account || isEstimatingFee || !targetAccount.trim()) {
      return;
    }

    setIsEstimatingFee(true);
    setLocalIsEstimatingFee(true);

    // Ensure the form state is updated for fee estimation
    setTransferFormState({ targetAccountAddress: targetAccount });

    try {
      // Use a small delay to ensure state is updated
      await new Promise((resolve) => setTimeout(resolve, 100));

      const result = await calculateFeeEstimate?.();

      if (result && "Accept" in result.result.result) {
        const fee = result.fee + 100; // Add buffer as per original comment
        setTransferFormState({ maxFee: fee.toString() });
        return fee;
      } else {
        console.error("Fee estimation rejected:", result);
        throw new Error("Could not estimate transfer fee");
      }
    } catch (e: any) {
      console.error("Fee estimation error:", e);
      throw e;
    } finally {
      setIsEstimatingFee(false);
      setLocalIsEstimatingFee(false);
    }
  };

  const estimateFee = async () => {
    return estimateFeeWithTargetAccount(transferFormState.targetAccountAddress);
  };

  const handleFormSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!account) return;

    // Check if target account is filled
    if (!transferFormState.targetAccountAddress.trim()) {
      setPopup({ title: "Missing information", error: true, message: "Please enter the target account public key" });
      return;
    }

    // Check if NFTs are selected (if not pre-selected)
    if (!preSelectedNftId && transferFormState.nfts.length === 0) {
      setPopup({ title: "Missing NFTs", error: true, message: "Please select at least one NFT to transfer" });
      return;
    }

    // Proceed to confirmation step (fee estimation will happen there)
    setCurrentStep("confirmation");
  };

  const onTransfer = async () => {
    if (!account || !sourceAccount || !feePayerAccount) {
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

        // Auto-close after 10 seconds
        const timeoutId = setTimeout(() => {
          handleAutoCloseAfterSuccess();
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
    resetState(preSelectedNftId, preSelectedResourceAddress);
    props.handleClose?.();
  };

  const handleAutoCloseAfterSuccess = () => {
    resetState(preSelectedNftId, preSelectedResourceAddress);
    props.handleClose?.();
    props.onSendComplete?.();
  };

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
      nfts: nftsSelected.length > 0,
    });
    if (nftsSelected.length === 0) {
      return;
    }

    setTransferFormState({
      nfts: nftsSelected.map((item) => item.nft_id),
      resourceAddress: nftsSelected[0].resource_address,
    });
  };

  const handlePayerAccountChange = (event: SelectChangeEvent) => {
    const value = event.target.value;

    if (value != "") {
      setValidity({
        payerAccount: true,
      });

      setTransferFormState({
        payerAccount: value,
      });
    }
  };

  // Handle target account changes for auto fee estimation
  useEffect(() => {
    const targetAccount = transferFormState.targetAccountAddress;
    if (targetAccount.trim() && targetAccount.match(/^[0-9a-fA-F]+$/)) {
      // Small delay to let state update, then estimate fee
      const timeoutId = setTimeout(() => {
        estimateFeeWithTargetAccount(targetAccount).catch(() => {
          // Fee estimation failed, but don't block the user
        });
      }, 500);

      return () => clearTimeout(timeoutId);
    }
  }, [transferFormState.targetAccountAddress]);

  useEffect(() => {
    if (loadedNfts !== undefined) {
      setAvailableNfts(loadedNfts);
    }
  }, [loadedNfts]);

  useEffect(() => {
    if (props.open && account) {
      // When dialog opens, ensure we have fresh state with correct NFT
      initializeFormState(preSelectedNftId, preSelectedResourceAddress, substateIdToString(account.component_address));
    }
  }, [props.open, preSelectedNftId, preSelectedResourceAddress, account?.component_address]);

  return (
    <Dialog open={props.open} onClose={handleClose} maxWidth="sm" fullWidth>
      <PopupTitle onClose={handleClose} title="Transfer NFT" />
      <DialogContent>
        {!account ? (
          <div style={{ padding: "20px", textAlign: "center" }}>
            <p>Please select an account first to transfer NFTs.</p>
          </div>
        ) : (
          <>
            {currentStep === "form" && (
              <FormStep
                account={account}
                accounts={accounts}
                availableNfts={availableNfts}
                preSelectedNftId={preSelectedNftId}
                isEstimatingFee={isEstimatingFee}
                onSubmit={handleFormSubmit}
                onCancel={handleClose}
                onNftsChange={handleNftsChange}
                onPayerAccountChange={handlePayerAccountChange}
              />
            )}

            {currentStep === "confirmation" && (
              <ConfirmationStep
                accounts={accounts}
                preSelectedNftId={preSelectedNftId}
                availableNfts={availableNfts}
                onBack={() => setCurrentStep("form")}
                onConfirm={onTransfer}
              />
            )}

            {currentStep === "result" && <ResultStep onClose={handleClose} />}
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
