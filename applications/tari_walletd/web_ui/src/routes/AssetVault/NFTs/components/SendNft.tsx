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

import PopupTitle from "@/components/PopupTitle";
import { useAccountsList } from "@api/hooks/useAccounts";
import { useNFTsList, useNftsTransfer } from "@api/hooks/useNfts";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import { SelectChangeEvent } from "@mui/material/Select";
import useAccountStore from "@store/accountStore";
import { useNftTransferStore } from "@store/nftTransferStore";
import {
  Account,
  ComponentAddressOrName,
  NonFungibleId,
  NonFungibleToken,
  ResourceAddress,
} from "@tari-project/ootle-ts-bindings";
import { substateIdToString } from "@utils/helpers";
import { FormEvent, useEffect, useMemo, useState } from "react";
import { formatNftTransferRejection, resolveSelectedNftTransferState } from "../nftTransferHelpers";
import ConfirmationStep from "../steps/ConfirmationStep";
import FormStep from "../steps/FormStep";
import ResultStep from "../steps/ResultStep";

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
      {/* Mounted only while open: the dialog loads the account's full NFT list, and one instance is rendered per
          NFT in the list. */}
      {open && (
        <TransferNftDialog
          open
          handleClose={() => setOpen(false)}
          onSendComplete={() => setOpen(false)}
          preSelectedNftId={nftId}
          preSelectedResourceAddress={resourceAddress}
        />
      )}
    </>
  );
}

export interface TransferNftDialogProps {
  open: boolean;
  onSendComplete?: () => void;
  handleClose: () => void;
  preSelectedNftId?: NonFungibleId;
  preSelectedResourceAddress?: ResourceAddress;
  preSelectedNfts?: NonFungibleToken[];
}

function getAccountSelector(account: Account): ComponentAddressOrName {
  return {
    ComponentAddress: account.component_address,
  };
}

export function TransferNftDialog(props: TransferNftDialogProps) {
  const { preSelectedNftId, preSelectedResourceAddress, preSelectedNfts } = props;
  const { account, setPopup } = useAccountStore();
  const hasBatchSelection = preSelectedNfts && preSelectedNfts.length > 0;

  const {
    currentStep,
    transferFormState,
    transferResult,
    setCurrentStep,
    setDisabled,
    setTransferFormState,
    setValidity,
    setTransferResult,
    resetState,
  } = useNftTransferStore();

  const [availableNfts, setAvailableNfts] = useState<NonFungibleToken[]>([]);
  const [isEstimatingFee, setIsEstimatingFee] = useState(false);

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
  const feeEstimateParams = useMemo(() => {
    return {
      dry_run: true,
      max_fee: 1n,
      nfts: transferFormState.nfts,
      source_account: sourceAccount!,
      target_account_address: transferFormState.targetAccountAddress,
      fee_payer_account: feePayerAccount!,
      resource_address: transferFormState.resourceAddress,
    };
  }, [
    transferFormState.nfts,
    sourceAccount,
    transferFormState.targetAccountAddress,
    feePayerAccount,
    transferFormState.resourceAddress,
  ]);

  const transferParams = useMemo(() => {
    return {
      nfts: transferFormState.nfts,
      source_account: sourceAccount!,
      target_account_address: transferFormState.targetAccountAddress,
      dry_run: false,
      max_fee: transferFormState.maxFee ? BigInt(transferFormState.maxFee) : 0n,
      fee_payer_account: feePayerAccount!,
      resource_address: transferFormState.resourceAddress,
    };
  }, [
    transferFormState.nfts,
    sourceAccount,
    transferFormState.targetAccountAddress,
    transferFormState.maxFee,
    feePayerAccount,
    transferFormState.resourceAddress,
  ]);

  // Fee estimation and transfer hooks
  const { mutateAsync: calculateFeeEstimate } = useNftsTransfer(feeEstimateParams);
  const { mutateAsync: sendTransferNftsTx } = useNftsTransfer(transferParams);

  const estimateFee = async () => {
    if (!account || isEstimatingFee || !transferFormState.targetAccountAddress.trim()) {
      return;
    }

    setIsEstimatingFee(true);

    try {
      const result = await calculateFeeEstimate?.();

      if (result && "Accept" in result.result.result) {
        setTransferFormState({ maxFee: result.fee.toString() });
        return result.fee;
      } else {
        console.error("Fee estimation rejected:", result);
        throw new Error(formatNftTransferRejection(result));
      }
    } catch (e: any) {
      console.error("Fee estimation error:", e);
      throw e;
    } finally {
      setIsEstimatingFee(false);
    }
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
    if (!preSelectedNftId && !hasBatchSelection && transferFormState.nfts.length === 0) {
      setPopup({ title: "Missing NFTs", error: true, message: "Please select at least one NFT to transfer" });
      return;
    }

    // If no fee is calculated yet, estimate it before proceeding
    if (!transferFormState.maxFee) {
      try {
        const fee = await estimateFee();
        if (fee === undefined) {
          return;
        }
      } catch (error) {
        console.error("Fee estimation failed:", error);
        setPopup({
          title: "Fee estimation failed",
          error: true,
          message: error instanceof Error ? error.message : String(error),
        });
        return;
      }
    }

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
      } else {
        setTransferResult({
          success: false,
          message: `Transfer failed: ${formatNftTransferRejection(result)}`,
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
    const wasSuccessful = transferResult?.success;
    resetState(preSelectedNftId, preSelectedResourceAddress);
    props.handleClose?.();
    if (wasSuccessful) {
      props.onSendComplete?.();
    }
  };

  const handleNftsChange = (event: SelectChangeEvent<string[]>) => {
    if (typeof event.target.value == "string") {
      return;
    }
    const nextState = resolveSelectedNftTransferState(event.target.value, availableNfts, preSelectedResourceAddress);
    setValidity({
      nfts: nextState.nfts.length > 0,
    });
    setTransferFormState(nextState);
  };

  const handlePayerAccountChange = (event: SelectChangeEvent) => {
    const value = event.target.value;

    if (value != "") {
      setValidity({
        payerAccount: true,
      });

      setTransferFormState({
        payerAccount: value,
        maxFee: "",
      });
    }
  };

  const filteredNfts = useMemo(() => {
    if (preSelectedResourceAddress && !preSelectedNftId) {
      return availableNfts.filter((nft) => nft.resource_address === preSelectedResourceAddress);
    }
    return availableNfts;
  }, [availableNfts, preSelectedResourceAddress, preSelectedNftId]);

  useEffect(() => {
    if (loadedNfts !== undefined) {
      setAvailableNfts(loadedNfts);
    }
  }, [loadedNfts]);

  // Seed the form from the props captured when the dialog opened. The NFT props are derived from the account's
  // NFT list, which the transfer invalidates, so re-seeding on their identity would wipe the result step out
  // from under the user the moment the refreshed list arrives.
  useEffect(() => {
    if (!props.open || !account) {
      return;
    }
    resetState(preSelectedNftId, preSelectedResourceAddress, substateIdToString(account.component_address));

    if (hasBatchSelection) {
      setTransferFormState({
        nfts: preSelectedNfts.map((nft) => nft.nft_id),
        resourceAddress: preSelectedNfts[0].resource_address,
      });
      setValidity({ nfts: true });
    }
  }, [props.open, account?.component_address]);

  return (
    <Dialog open={props.open} onClose={handleClose} maxWidth="sm" fullWidth>
      <PopupTitle
        onClose={handleClose}
        title={
          preSelectedNftId ? "Transfer NFT" : `Transfer NFTs${hasBatchSelection ? ` (${preSelectedNfts.length})` : ""}`
        }
      />
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
                availableNfts={filteredNfts}
                preSelectedNftId={preSelectedNftId}
                preSelectedNfts={preSelectedNfts}
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
                availableNfts={filteredNfts}
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
