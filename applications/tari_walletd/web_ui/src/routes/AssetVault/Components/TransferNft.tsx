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

import { FormEvent, useEffect, useState } from "react";
import { Form } from "react-router-dom";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import Box from "@mui/material/Box";
import { useTheme } from "@mui/material/styles";
import useAccountStore from "../../../store/accountStore";
import type {
  Account,
  ComponentAddressOrName,
  ResourceAddress,
  NonFungibleId,
  NonFungibleToken,
  TransactionResult,
} from "@tari-project/typescript-bindings";
import Select from "@mui/material/Select";
import MenuItem from "@mui/material/MenuItem";
import Checkbox from "@mui/material/Checkbox";
import ListItemText from "@mui/material/ListItemText";
import { InputLabel } from "@mui/material";
import { SelectChangeEvent } from "@mui/material/Select/Select";
import { useListNfts, useNftsTransfer } from "../../../api/hooks/useNfts";
import { substateIdToString } from "../../../utils/helpers";
import { useAccountsList } from "../../../api/hooks/useAccounts";

export default function TransferNft() {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button variant="outlined" onClick={() => setOpen(true)}>
        Transfer NFT
      </Button>
      <TransferNftDialog open={open} handleClose={() => setOpen(false)} onSendComplete={() => setOpen(false)} />
    </>
  );
}

export interface TransferNftDialogProps {
  open: boolean;
  onSendComplete?: () => void;
  handleClose: () => void;
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

export function TransferNftDialog(props: TransferNftDialogProps) {
  const INITIAL_VALUES = {
    payerAccount: "",
    nfts: [] as NonFungibleId[],
    targetAccountPublicKey: "",
    maxFee: "",
    resourceAddress: "" as ResourceAddress,
  };
  const [disabled, setDisabled] = useState(false);
  const [transferFormState, setTransferFormState] = useState(INITIAL_VALUES);
  const [validity, setValidity] = useState<object>({
    payerAccount: true,
    nfts: false,
    targetAccountPublicKey: false,
  });
  const [allValid, setAllValid] = useState(false);

  const { account, setPopup } = useAccountStore();
  if (!account) {
    return null;
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
    setTransferFormState({
      ...transferFormState,
      [e.target.name]: e.target.value,
    });
    if (validity[e.target.name as keyof object] !== undefined) {
      setValidity({
        ...validity,
        [e.target.name]: e.target.validity.valid,
      });
    }
  }

  const onTransfer = async (e: FormEvent) => {
    e.preventDefault();
    if (!account) {
      return;
    }

    setDisabled(true);

    if (!isNaN(parseInt(transferFormState.maxFee))) {
      sendTransferNftsTx?.()
        .then()
        .then((result) => {
          if (!("Accept" in result.result.result)) {
            setPopup({
              title: "Sending transfer transaction failed",
              error: true,
              message: JSON.stringify(
                unionGet(result.result.result, "Reject" as keyof TransactionResult) ||
                  unionGet(result.result.result, "AcceptFeeRejectRest" as keyof TransactionResult)?.[1],
              ),
            });
            return;
          }
          setTransferFormState(INITIAL_VALUES);
          props.onSendComplete?.();
          setPopup({ title: "NFT transfer transaction", error: false, message: "Sent successfully!" });
        })
        .catch((e) => {
          setPopup({ title: "NFT transfer failed", error: true, message: e.message });
        })
        .finally(() => {
          setDisabled(false);
        });
    } else {
      calculateFeeEstimate?.()
        .then((result) => {
          if (!("Accept" in result.result.result)) {
            setPopup({
              title: "Fee estimate failed",
              error: true,
              // TODO: fix this
              message: JSON.stringify(
                unionGet(result.result.result, "Reject" as keyof TransactionResult) ||
                  unionGet(result.result.result, "AcceptFeeRejectRest" as keyof TransactionResult)?.[1],
              ),
            });
            return;
          }

          // Simple fix for the estimated fee differing between the dry-run and non-dry-run transactions.
          // Since fees are charged for the transaction byte size and for confidential transfers, the rangeproof
          // may differ in length and, therefore in fees. The fees may differ typically by 2/3, this more than
          // accounts for that. See https://github.com/tari-project/tari-ootle/issues/1312
          // TODO: remove once this is no longer an issue
          const fee = result.fee + 100;
          setTransferFormState({ ...transferFormState, maxFee: fee.toString() });
        })
        .catch((e) => {
          setPopup({ title: "Fee estimate failed", error: true, message: e.message });
        })
        .finally(() => {
          setDisabled(false);
        });
    }
  };

  const handleClose = () => {
    // rest states
    setTransferFormState(INITIAL_VALUES);
    setDisabled(false);
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
    if (transferFormState.payerAccount != "") {
      setTransferFormState({
        ...transferFormState,
        payerAccount: substateIdToString(account.address),
      });
    }
  }, [open]);

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

  return (
    <Dialog open={props.open} onClose={handleClose}>
      <DialogTitle>Transfer NFT</DialogTitle>
      <DialogContent className="dialog-content">
        <Form onSubmit={onTransfer} className="flex-container-vertical" style={{ paddingTop: theme.spacing(1) }}>
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
                {accounts.map((account, i) => (
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
            label="Fee"
            value={transferFormState.maxFee}
            placeholder="Enter fee or press Estimate Fee to calculate"
            onChange={setFormValue}
            disabled={disabled}
            style={{ flexGrow: 1 }}
          />

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
            {availableNfts.map((nft, i) => (
              <MenuItem key={i} value={JSON.stringify(nft.nft_id)}>
                <Checkbox
                  checked={transferFormState.nfts.some((id) => nftIdToString(id) == nftIdToString(nft.nft_id))}
                />
                <ListItemText primary={nftIdToString(nft.nft_id)} />
              </MenuItem>
            ))}
          </Select>

          <Box
            className="flex-container"
            style={{
              justifyContent: "flex-end",
            }}
          >
            <Button variant="outlined" onClick={handleClose} disabled={disabled}>
              Cancel
            </Button>
            <Button variant="contained" type="submit" disabled={disabled || !allValid}>
              {isNaN(parseInt(transferFormState.maxFee)) ? "Estimate fee" : "Send"}
            </Button>
          </Box>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

function unionGet<T extends object>(object: T, key: keyof T): T[keyof T] | null {
  return key in object ? object[key] : null;
}
