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

import React, { useEffect, useState } from "react";
import { Form } from "react-router-dom";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import Box from "@mui/material/Box";
import { useAccountsList } from "../../../api/hooks/useAccounts";
import { useTheme } from "@mui/material/styles";
import useAccountStore from "../../../store/accountStore";
import { FormControl, InputLabel, MenuItem, Select, SelectChangeEvent } from "@mui/material";
import { useKeysList } from "../../../api/hooks/useKeys";
import { validatorsClaimFees, validatorsGetFees } from "../../../utils/json_rpc";
import {
  AccountInfo,
  getRejectReasonFromTransactionResult,
  GetValidatorFeesResponse,
  rejectReasonToString,
  substateIdToString,
} from "@tari-project/typescript-bindings";
import { FileContent } from "use-file-picker/types";
import { toHexString } from "../../../utils/helpers";

interface FormState {
  account: string | null;
  fee: number | null;
  shards: Array<number>;
  keyIndex: number | null;
}

const INITIAL_VALUES = {
  account: null,
  fee: null,
  shards: [],
  keyIndex: null,
};

const INITIAL_VALIDITY = {
  key: false,
  shard: false,
};

export default function ClaimFees() {
  const [open, setOpen] = useState(false);
  const [disabled, setDisabled] = useState(false);
  const [isValid, setIsValid] = useState(false);
  const [formState, setFormState] = useState<FormState>(INITIAL_VALUES);
  const [scannedFees, setScannedFees] = useState<GetValidatorFeesResponse | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [validity, setValidity] = useState<{ [key: string]: boolean }>(INITIAL_VALIDITY);
  const { data: dataAccountsList } = useAccountsList(0, 10);
  const { data: dataKeysList } = useKeysList();
  const { setPopup } = useAccountStore();

  const theme = useTheme();

  const isFormFilled = () => {
    return validity.key && validity.shard;
  };

  useEffect(() => {
    setIsValid(!isFormFilled());
  }, [validity]);

  const onClaimFeesKeyChange = (e: SelectChangeEvent<number>) => {
    if (!dataKeysList) {
      return;
    }
    const keyIndex = +e.target.value;
    if (keyIndex === formState.keyIndex) {
      return;
    }
    const selected_account = dataAccountsList?.accounts.find(
      (account: AccountInfo) => account.account.key_index === keyIndex,
    );
    const account = selected_account?.account.address ? substateIdToString(selected_account!.account.address) : null;
    setScannedFees(null);
    setFormState({
      ...formState,
      keyIndex,
      account,
    });
  };

  function setFormValue(e: React.ChangeEvent<HTMLInputElement>) {
    setFormState({
      ...formState,
      [e.target.name]: e.target.value,
    });
    if (validity[e.target.name as keyof object] !== undefined && "validity" in e.target) {
      setValidity({
        ...validity,
        [e.target.name]: e.target.validity.valid,
      });
    }
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

    const isFeeSet = Boolean(formState.fee) && !isNaN(formState.fee || 0);
    setDisabled(true);
    validatorsClaimFees({
      account: formState.account ? { ComponentAddress: formState.account } : null,
      claim_key_index: formState.keyIndex,
      max_fee: 3000,
      shards: formState.shards,
      dry_run: !isFeeSet,
    })
      .then((resp) => {
        if ("Accept" in resp.result.result) {
          if (isFeeSet) {
            setFormState(INITIAL_VALUES);
            setValidity(INITIAL_VALIDITY);
            setOpen(false);
            setPopup({ title: "Claim successful", error: false });
          } else {
            setFormState({ ...formState, fee: resp.fee });
          }
        } else {
          setPopup({
            title: "Claim failed",
            error: true,
            message: rejectReasonToString(getRejectReasonFromTransactionResult(resp.result.result)),
          });
        }
      })
      .catch((e) => {
        setPopup({ title: "Claim failed", error: true, message: e.message });
      })
      .finally(() => {
        // Previous value of disabled
        setDisabled(disabled);
      });
  };

  const handleClickOpen = () => {
    setOpen(true);
  };

  const handleClose = () => {
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
        account_or_key: { KeyIndex: formState.keyIndex },
        shard_group: null,
      });
      setScannedFees(fees);
      setFormState({ ...formState, shards: Object.entries(fees.fees).map(([shard, _info]) => +shard) });
    } catch (e: any) {
      setPopup({ title: "Scan failed", error: true, message: e.message });
    } finally {
      setIsLoading(false);
    }
  };

  const formatKey = ([index, publicKey, _isActive]: [number, string, boolean]) => {
    let account = dataAccountsList?.accounts.find((account: AccountInfo) => account.account.key_index === +index);
    return (
      <div>
        <b>{index}</b> {publicKey}
        <br></br>Account <i>{account?.account.name || "<None>"}</i>
      </div>
    );
  };

  return (
    <div>
      <Button variant="outlined" onClick={handleClickOpen}>
        Claim Fees
      </Button>
      <Dialog open={open} onClose={handleClose}>
        <DialogTitle>Claim Fees</DialogTitle>
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
                {dataKeysList?.keys.map((account: [number, string, boolean]) => (
                  <MenuItem key={account[0]} value={account[0]}>
                    {formatKey(account)}
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
                Found fees in {Object.entries(scannedFees.fees).length} shards. Total:{" "}
                {Object.entries(scannedFees.fees)
                  .map(([_shard, info]) => info.amount)
                  .reduce((acc, amt) => acc + amt, 0)}{" "}
                XTR
              </Box>
            ) : null}
            <TextField
              name="fee"
              label="Max Transaction Fee"
              placeholder="Press Estimate Fee to calculate"
              value={formState.fee === null ? "" : formState.fee}
              onChange={setFormValue}
              style={{ flexGrow: 1 }}
              disabled={disabled}
            />

            <Box
              className="flex-container"
              style={{
                justifyContent: "flex-end",
              }}
            >
              <Button variant="outlined" onClick={handleClose} disabled={disabled}>
                Cancel
              </Button>
              <Button variant="contained" type="submit" disabled={disabled || !isValid}>
                {formState.fee ? "Claim" : "Estimate fee"}
              </Button>
            </Box>
          </Form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
