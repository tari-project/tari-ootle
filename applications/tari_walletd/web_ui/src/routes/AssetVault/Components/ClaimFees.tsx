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
import {
  FormControl,
  FormControlLabel,
  InputLabel,
  MenuItem,
  Select,
  SelectChangeEvent,
  Typography,
} from "@mui/material";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CheckBox from "@mui/material/Checkbox";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import { useTheme } from "@mui/material/styles";
import TextField from "@mui/material/TextField";
import useAccountStore from "@store/accountStore";
import {
  getRejectReasonFromTransactionResult,
  GetValidatorFeesResponse,
  rejectReasonToString,
  TransactionResult,
} from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";
import { formatCurrency } from "@utils/helpers";
import { validatorsClaimFees, validatorsGetFees } from "@utils/json_rpc";
import React, { useState } from "react";
import { Form } from "react-router-dom";
import {
  canSubmitClaimFees,
  findAccountAddressForClaimKey,
  getAccountClaimKeys,
  parseOptionalFee,
  sumScannedFees,
} from "./claimFeesHelpers";

interface FormState {
  account: string | null;
  fee: string;
  shards: Array<number>;
  keyIndex: number | null;
  outputToRevealed: boolean;
}

const INITIAL_VALUES: FormState = {
  account: null,
  fee: "",
  shards: [],
  keyIndex: null,
  outputToRevealed: false,
};

export default function ClaimFees() {
  const [open, setOpen] = useState(false);
  const [disabled, setDisabled] = useState(false);
  const [formState, setFormState] = useState<FormState>(INITIAL_VALUES);
  const [scannedFees, setScannedFees] = useState<GetValidatorFeesResponse | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const { data: dataAccountsList } = useAccountsList(0, 1000);
  const { setPopup } = useAccountStore();

  const theme = useTheme();

  // A validator's fees are claimed with the key of a derived account
  const claimKeys = getAccountClaimKeys(dataAccountsList?.accounts);
  const parsedFee = parseOptionalFee(formState.fee);
  const canSubmit = canSubmitClaimFees(formState.keyIndex, formState.shards, formState.fee);

  const onClaimFeesKeyChange = (e: SelectChangeEvent<number>) => {
    if (!dataAccountsList) {
      return;
    }
    const keyIndex = +e.target.value;
    if (keyIndex === formState.keyIndex) {
      return;
    }
    const account = findAccountAddressForClaimKey(claimKeys, keyIndex);
    setScannedFees(null);
    setFormState({
      ...formState,
      keyIndex,
      account,
      shards: [],
      fee: "",
    });
  };

  function setFormValue(e: React.ChangeEvent<HTMLInputElement>) {
    setFormState({
      ...formState,
      [e.target.name]: e.target.value,
    });
  }

  const onClaim = async (e: React.FormEvent) => {
    e.preventDefault();
    if (formState.keyIndex === null) {
      console.warn("Claim key is not selected");
      return;
    }
    if (!formState.shards.length) {
      console.warn("No shards selected");
      return;
    }
    if (parsedFee.kind === "invalid") {
      return;
    }

    const isFeeSet = parsedFee.kind === "valid";
    setDisabled(true);
    validatorsClaimFees({
      account: formState.account ? { ComponentAddress: formState.account } : null,
      claim_key_index: formState.keyIndex,
      max_fee: isFeeSet ? parsedFee.value : 1n,
      shards: formState.shards,
      dry_run: !isFeeSet,
      output_to_revealed: formState.outputToRevealed,
    })
      .then((resp) => {
        if ("Accept" in resp.result.result) {
          if (isFeeSet) {
            setFormState(INITIAL_VALUES);
            setScannedFees(null);
            setOpen(false);
            setPopup({ title: "Claim successful", error: false });
          } else {
            setFormState((prev) => ({ ...prev, fee: resp.fee.toString() }));
          }
        } else {
          setPopup({
            title: "Claim failed",
            error: true,
            message: rejectReasonToString(
              getRejectReasonFromTransactionResult(resp.result.result as TransactionResult),
            ),
          });
        }
      })
      .catch((e) => {
        setPopup({ title: "Claim failed", error: true, message: e.message });
      })
      .finally(() => {
        setDisabled(false);
      });
  };

  const handleClickOpen = () => {
    setOpen(true);
  };

  const handleClose = () => {
    setFormState(INITIAL_VALUES);
    setScannedFees(null);
    setOpen(false);
  };

  const handleScanForFees = async (e: React.MouseEvent) => {
    e.preventDefault();
    if (formState.keyIndex === null) {
      console.warn("Claim key is not selected");
      return;
    }
    setIsLoading(true);
    try {
      const fees = await validatorsGetFees({
        account_or_key: { KeyId: { Derived: { key_branch: "account", index: BigInt(formState.keyIndex) } } },
        shard_group: null,
      });
      setScannedFees(fees);
      setFormState((prev) => ({
        ...prev,
        shards: Object.keys(fees.fees).map((shard) => +shard),
        fee: "",
      }));
    } catch (e: any) {
      setPopup({ title: "Scan failed", error: true, message: e.message });
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div>
      <Button variant="outlined" onClick={handleClickOpen}>
        Claim Fees
      </Button>
      <Dialog open={open} onClose={handleClose}>
        <PopupTitle onClose={handleClose} title="Claim Fees" />
        <DialogContent className="dialog-content">
          <Form onSubmit={onClaim} className="flex-container-vertical" style={{ paddingTop: theme.spacing(1) }}>
            <FormControl>
              <InputLabel id="key">Key</InputLabel>
              <Select
                labelId="key"
                name="keyIndex"
                label="key"
                value={formState.keyIndex !== null ? formState.keyIndex : ""}
                onChange={onClaimFeesKeyChange}
                style={{ flexGrow: 1, minWidth: "200px" }}
                disabled={disabled}
              >
                {claimKeys.map(({ index, account }) => (
                  <MenuItem key={index.toString()} value={index.toString()}>
                    <div>
                      <b>Derived:{index.toString()}</b> {account.owner_public_key}
                      <br></br>Account <i>{account.name || "<None>"}</i>
                    </div>
                  </MenuItem>
                ))}
              </Select>
            </FormControl>
            <Button
              variant="outlined"
              onClick={handleScanForFees}
              disabled={disabled || formState.keyIndex === null || isLoading}
            >
              {isLoading ? "Scanning..." : "Scan for Fees"}
            </Button>
            {scannedFees?.fees ? (
              <Box>
                Found fees in {Object.keys(scannedFees.fees).length} shards. Total:{" "}
                {formatCurrency(sumScannedFees(scannedFees.fees), XTR_CURRENCY)}
              </Box>
            ) : null}
            <InputLabel>Max Fee</InputLabel>
            <TextField
              name="fee"
              placeholder="Press Estimate Fee to calculate"
              value={formState.fee}
              onChange={setFormValue}
              style={{ flexGrow: 1 }}
              disabled={disabled}
              error={parsedFee.kind === "invalid"}
              helperText={parsedFee.kind === "invalid" ? "Enter a positive integer fee in micro-TARI" : undefined}
            />

            <FormControlLabel
              control={
                <CheckBox
                  name="outputToRevealed"
                  checked={formState.outputToRevealed}
                  onChange={(e) => setFormState({ ...formState, outputToRevealed: e.target.checked })}
                  disabled={disabled}
                />
              }
              label="Claim to revealed funds"
            />
            {formState.outputToRevealed ? (
              <Typography color="warning.main">
                ⚠️ Warning: Revealed funds are visible on the blockchain and can be viewed by anyone. By default, fees
                are claimed into a stealth UTXO that is private to your wallet.
              </Typography>
            ) : null}

            <Box
              className="flex-container"
              style={{
                justifyContent: "flex-end",
              }}
            >
              <Button variant="outlined" onClick={handleClose} disabled={disabled}>
                Cancel
              </Button>
              <Button variant="contained" type="submit" disabled={disabled || !canSubmit}>
                {parsedFee.kind === "valid" ? "Claim" : "Estimate fee"}
              </Button>
            </Box>
          </Form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
