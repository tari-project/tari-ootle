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

import { FormEvent } from "react";
import { Form } from "react-router-dom";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Select from "@mui/material/Select";
import MenuItem from "@mui/material/MenuItem";
import Checkbox from "@mui/material/Checkbox";
import ListItemText from "@mui/material/ListItemText";
import { Divider, InputLabel, Stack } from "@mui/material";
import { SelectChangeEvent } from "@mui/material/Select/Select";
import type { NonFungibleId, NonFungibleToken, Account } from "@tari-project/typescript-bindings";
import { substateIdToString, formatCurrency, displayNftId } from "@utils/helpers";
import { useNftTransferStore } from "@store/nftTransferStore";
import { validateOotleAddress } from "@tari-project/typescript-bindings/dist/helpers/ootleAddress";

interface FormStepProps {
  account: Account;
  accounts: Array<{ account: Account }> | undefined;
  availableNfts: NonFungibleToken[];
  preSelectedNftId?: NonFungibleId;
  isEstimatingFee: boolean;
  onSubmit: (e: FormEvent) => void;
  onCancel: () => void;
  onNftsChange: (event: SelectChangeEvent<string[]>) => void;
  onPayerAccountChange: (event: SelectChangeEvent) => void;
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

export default function FormStep({
  accounts,
  availableNfts,
  preSelectedNftId,
  isEstimatingFee,
  onSubmit,
  onCancel,
  onNftsChange,
  onPayerAccountChange,
}: FormStepProps) {
  const { transferFormState, disabled, updateFormValue } = useNftTransferStore();

  const setFormValue = (e: React.ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target;
    updateFormValue(name, value, e.target.validity.valid);
  };

  return (
    <Form onSubmit={onSubmit}>
      <Stack direction="column" spacing={2} sx={{ py: 2 }}>
        {accounts && (
          <>
            <InputLabel id="select-payer-account">Account (to pay fees)</InputLabel>
            <Select
              id="select-payer-account"
              name="payerAccount"
              disabled={disabled}
              displayEmpty
              value={
                transferFormState.payerAccount ||
                substateIdToString(accounts.find((a) => a.account.is_default)?.account.component_address) ||
                ""
              }
              onChange={onPayerAccountChange}
              variant="outlined"
            >
              {accounts.map((account) => (
                <MenuItem key={account.account.name} value={substateIdToString(account.account.component_address)}>
                  {account.account.name} {account.account.is_default ? "(default)" : ""}
                </MenuItem>
              ))}
            </Select>
          </>
        )}

        <TextField
          name="targetAccountAddress"
          label="To Account address"
          value={transferFormState.targetAccountAddress}
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
                ? formatCurrency(parseInt(transferFormState.maxFee))
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
              onChange={onNftsChange}
              renderValue={(selected) => selected.map((item) => item).join(", ")}
            >
              {availableNfts.map((nft, index) => (
                <MenuItem key={index} value={JSON.stringify(nft.nft_id)}>
                  <Checkbox
                    checked={transferFormState.nfts.some((id) => nftIdToString(id) == nftIdToString(nft.nft_id))}
                  />
                  <ListItemText primary={displayNftId(nft.nft_id)} />
                </MenuItem>
              ))}
            </Select>
          </>
        ) : (
          <TextField
            label="Selected NFT"
            value={displayNftId(preSelectedNftId)}
            disabled
            variant="outlined"
            style={{ flexGrow: 1 }}
          />
        )}
        <Divider />
        <Stack direction="row" justifyContent="space-between" sx={{ mt: 3 }}>
          <Button variant="outlined" onClick={onCancel} disabled={disabled}>
            Cancel
          </Button>
          <Button
            variant="contained"
            type="submit"
            disabled={disabled || !validateOotleAddress(transferFormState.targetAccountAddress)}
          >
            Continue
          </Button>
        </Stack>
      </Stack>
    </Form>
  );
}
