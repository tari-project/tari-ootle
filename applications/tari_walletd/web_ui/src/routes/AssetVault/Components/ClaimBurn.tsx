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
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select, { SelectChangeEvent } from "@mui/material/Select";
import { useTheme } from "@mui/material/styles";
import TextField from "@mui/material/TextField";
import useAccountStore from "@store/accountStore";
import type { AccountInfo, ComponentAddress } from "@tari-project/ootle-ts-bindings";
import { accountsClaimBurn, transactionsWaitResult } from "@utils/json_rpc";
import { FormEvent, useState } from "react";
import { Form } from "react-router";

type FormState = {
  account: ComponentAddress;
  claimProof: string;
  fee: string;
  is_valid_json: boolean;
  filled: boolean;
  disabled: boolean;
};

const INITIAL_FORM_STATE: FormState = {
  account: "",
  claimProof: "",
  fee: "",
  is_valid_json: false,
  filled: false,
  disabled: false,
};

export default function ClaimBurn() {
  const [open, setOpen] = useState(false);
  const [claimBurnFormState, setClaimBurnFormState] = useState<FormState>(INITIAL_FORM_STATE);

  const { data: accountsList } = useAccountsList(0, 10);
  const { setPopup } = useAccountStore();

  const onClaimBurnKeyChange = (e: SelectChangeEvent) => {
    setClaimBurnFormState({
      ...claimBurnFormState,
      account: e.target.value,
      filled: claimBurnFormState.is_valid_json && claimBurnFormState.fee !== "" && e.target.value !== "",
    });
  };

  const theme = useTheme();

  const onClaimBurnClaimProofChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    // We have to check if the claim proof is valid JSON
    try {
      JSON.parse(e.target.value);
      setClaimBurnFormState({
        ...claimBurnFormState,
        [e.target.name]: e.target.value,
        is_valid_json: true,
        filled: claimBurnFormState.fee !== "" && e.target.value !== "",
      });
    } catch {
      setClaimBurnFormState({
        ...claimBurnFormState,
        [e.target.name]: e.target.value,
        is_valid_json: false,
        filled: false,
      });
    }
  };

  const onClaimBurnFeeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setClaimBurnFormState({
      ...claimBurnFormState,
      [e.target.name]: e.target.value,
      filled: claimBurnFormState.is_valid_json && e.target.value !== "",
    });
  };

  const onClaimBurn = async (e: FormEvent) => {
    e.preventDefault();
    try {
      setClaimBurnFormState({ ...claimBurnFormState, disabled: true });
      const resp = await accountsClaimBurn({
        account: { ComponentAddress: claimBurnFormState.account },
        claim_proof: JSON.parse(claimBurnFormState.claimProof),
        max_fee: +claimBurnFormState.fee,
      });
      const waitResp = await transactionsWaitResult({ transaction_id: resp.transaction_id, timeout_secs: 30 });
      if (waitResp.status != "Accepted") {
        throw new Error(`Transaction not accepted: ${waitResp.status}`);
      }
      setOpen(false);
      setPopup({ title: "Claimed", error: false });
      setClaimBurnFormState(INITIAL_FORM_STATE);
    } catch (e: any) {
      setClaimBurnFormState({ ...claimBurnFormState, disabled: false });
      setPopup({ title: "Claim burn failed: " + e.message, error: true });
    }
  };

  const handleClickOpen = () => {
    setClaimBurnFormState({ ...claimBurnFormState, disabled: false });
    setOpen(true);
  };

  const handleClose = () => {
    setOpen(false);
  };

  return (
    <div>
      <Button variant="outlined" onClick={handleClickOpen}>
        Claim Burn
      </Button>
      <Dialog open={open} onClose={handleClose}>
        <PopupTitle onClose={handleClose} title="Claim Burn" />
        <DialogContent className="dialog-content">
          <Form onSubmit={onClaimBurn} className="flex-container-vertical" style={{ paddingTop: theme.spacing(1) }}>
            <FormControl>
              <InputLabel id="key">Key</InputLabel>
              <Select
                labelId="key"
                name="key"
                label="Key"
                value={claimBurnFormState.account}
                onChange={onClaimBurnKeyChange}
                style={{ flexGrow: 1, minWidth: "200px" }}
                disabled={claimBurnFormState.disabled}
              >
                {accountsList?.accounts?.map((account: AccountInfo, i: number) => (
                  <MenuItem key={i} value={account.account.component_address}>
                    <div>
                      <i>{account.account.name}</i>
                    </div>
                  </MenuItem>
                ))}
              </Select>
            </FormControl>
            <TextField
              name="claimProof"
              label="Claim Proof"
              value={claimBurnFormState.claimProof}
              onChange={onClaimBurnClaimProofChange}
              style={{ flexGrow: 1 }}
              disabled={claimBurnFormState.disabled}
            />
            <TextField
              name="fee"
              label="Fee"
              value={claimBurnFormState.fee}
              onChange={onClaimBurnFeeChange}
              style={{ flexGrow: 1 }}
              disabled={claimBurnFormState.disabled}
            />
            <Box
              className="flex-container"
              style={{
                justifyContent: "flex-end",
              }}
            >
              <Button variant="outlined" onClick={handleClose} disabled={claimBurnFormState.disabled}>
                Cancel
              </Button>
              <Button
                variant="contained"
                type="submit"
                disabled={!claimBurnFormState.filled || claimBurnFormState.disabled}
              >
                Claim Burn
              </Button>
            </Box>
          </Form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
