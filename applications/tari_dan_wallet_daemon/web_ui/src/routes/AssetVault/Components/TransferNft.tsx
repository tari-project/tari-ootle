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

import {FormEvent, useEffect, useState} from "react";
import {Form} from "react-router-dom";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import Box from "@mui/material/Box";
import {useAccountsGetBalances} from "../../../api/hooks/useAccounts";
import {useTheme} from "@mui/material/styles";
import useAccountStore from "../../../store/accountStore";
import {ResourceAddress, ResourceType, substateIdToString,} from "@tari-project/typescript-bindings";
import Select from "@mui/material/Select";
import MenuItem from "@mui/material/MenuItem";
import Checkbox from "@mui/material/Checkbox";
import ListItemText from "@mui/material/ListItemText";
import {InputLabel} from "@mui/material";
import {SelectChangeEvent} from "@mui/material/Select/Select";
import {useListNfts} from "../../../api/hooks/useNfts";
import type {NonFungibleId} from "@tari-project/typescript-bindings/dist";

const XTR2 = "resource_0101010101010101010101010101010101010101010101010101010101010101";

export default function TransferNft() {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button variant="outlined" onClick={() => setOpen(true)}>
        Transfer NFT
      </Button>
      <TransferNftDialog
        open={open}
        handleClose={() => setOpen(false)}
        onSendComplete={() => setOpen(false)}
        resource_type="Confidential"
        resource_address={XTR2}
      />
    </>
  );
}

export interface TransferNftDialogProps {
  open: boolean;
  resource_address?: ResourceAddress;
  resource_type?: ResourceType;
  onSendComplete?: () => void;
  handleClose: () => void;
}

interface NftListItem {
  id: NonFungibleId,
  name: string,
}

function nftIdToString(nftId: NonFungibleId): string {
  const key = Object.keys(nftId)[0];
  // @ts-ignore
  return nftId[key].toString();
}

export function TransferNftDialog(props: TransferNftDialogProps) {
  const INITIAL_VALUES = {
    nftIds: [],
    targetAccountPublicKey: "",
    maxFee: "",
  };
  const [disabled, setDisabled] = useState(false);
  const [transferFormState, setTransferFormState] = useState(INITIAL_VALUES);
  const [validity, setValidity] = useState<object>({
    nftIds: false,
    targetAccountPublicKey: false,
  });
  const [allValid, setAllValid] = useState(false);

  const { account, setPopup } = useAccountStore();
  if (!account) {
    return null;
  }

  const theme = useTheme();

  const { data } = useAccountsGetBalances(substateIdToString(account.address));

  // const { mutateAsync: calculateFeeEstimate } = useAccountsTransfer({ ...transfer, dry_run: true, max_fee: 3000 });

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

  // const onTransfer = async (e: FormEvent) => {
  //   e.preventDefault();
  //   if (!account) {
  //     return;
  //   }
  //
  //   setDisabled(true);
  //   if (!isNaN(parseInt(transferFormState.maxFee))) {
  //     sendIt?.()
  //       .then(() => {
  //         setTransferFormState(INITIAL_VALUES);
  //         props.onSendComplete?.();
  //         setPopup({ title: "Send successful", error: false });
  //       })
  //       .catch((e) => {
  //         setPopup({ title: "Send failed", error: true, message: e.message });
  //       })
  //       .finally(() => {
  //         setDisabled(false);
  //       });
  //   } else {
  //     // calculateFeeEstimate?.()
  //       .then((result) => {
  //         if (!("Accept" in result.result.result)) {
  //           setPopup({
  //             title: "Fee estimate failed",
  //             error: true,
  //             // TODO: fix this
  //             message: JSON.stringify(
  //               unionGet(result.result.result, "Reject" as keyof TransactionResult) ||
  //                 unionGet(result.result.result, "AcceptFeeRejectRest" as keyof TransactionResult)?.[1],
  //             ),
  //           });
  //           return;
  //         }
  //         // Simple fix for the estimated fee differing between the dry-run and non-dry-run transactions.
  //         // Since fees are charged for the transaction byte size and for confidential transfers, the rangeproof
  //         // may differ in length and, therefore in fees. The fees may differ typically by 2/3, this more than
  //         // accounts for that. See https://github.com/tari-project/tari-dan/issues/1312
  //         // TODO: remove once this is no longer an issue
  //         const fee = result.fee + 100;
  //         setTransferFormState({ ...transferFormState, maxFee: fee.toString() });
  //       })
  //       .catch((e) => {
  //         setPopup({ title: "Fee estimate failed", error: true, message: e.message });
  //       })
  //       .finally(() => {
  //         setDisabled(false);
  //       });
  //   }
  // };

  const onTransfer = async (e: FormEvent) => {
    e.preventDefault();
    if (!account) {
      return;
    }

    console.log("Transfer data:", transferFormState);
  }

  const handleClose = () => {
    // rest states
    setNfts([]);
    setTransferFormState(INITIAL_VALUES);
    props.handleClose?.();
  };

  useEffect(() => {
    setAllValid(Object.values(validity).every((v) => v));
  }, [validity]);

  // NFT listing
  const {data: accountNfts} = useListNfts({
    account: account.name?.toString(),
    limit: 1000,
    offset: 0
  });
  const [nfts, setNfts] = useState<NftListItem[]>([]);
  const [availableNfts, setAvailableNfts] = useState<NftListItem[]>([]);
  useEffect(() => {
    if (accountNfts !== undefined) {
      setAvailableNfts(accountNfts.nfts.map(nft => {
        let nftName = "";

        const nftData: {Text: string}[][] = nft.data.Tag[1].Map;
        nftData.forEach(data => {
          const key = data[0].Text;
          const value = data[1].Text;
          if (key === "name") {
            nftName = value;
            return;
          }
        });
        if (nftName !== "") {
          return {id: nft.nft_id, name: nftName};
        }

        return {id: nft.nft_id, name: nftIdToString(nft.nft_id)};
      }))
    }
  }, [accountNfts]);

  const handleChange = (event: SelectChangeEvent<string[]>) => {
    if (typeof event.target.value == "string") {
      return;
    }
    const nftNamesSelected = event.target.value;
    const nftsSelected = nftNamesSelected.map(itemName => {
      return availableNfts.find(nft => {
        return nft.name === itemName;
      });
    })
        .filter(value => value !== undefined);

    setValidity({
      ...validity,
      ["nftIds"]: nftsSelected.length > 0,
    });

    setTransferFormState({
      ...transferFormState,
      [nftIds]: nftsSelected.map(item => item?.id),
    });

    setNfts(nftsSelected);
  };

  return (
    <Dialog open={props.open} onClose={handleClose}>
      <DialogTitle>Transfer NFT</DialogTitle>
      <DialogContent className="dialog-content">
        <Form onSubmit={onTransfer} className="flex-container-vertical" style={{ paddingTop: theme.spacing(1) }}>
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
              name="nftIds"
              id="nft-select"
              multiple
              value={nfts.map(value => value.name)}
              required
              disabled={disabled}
              onChange={handleChange}
              renderValue={(selected) => selected.map(item => item).join(', ')}
          >
            {availableNfts.map((nft) => (
                <MenuItem key={nftIdToString(nft.id)} value={nft.name}>
                  <Checkbox checked={nfts.includes(nft)} />
                  <ListItemText primary={nft.name} />
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
