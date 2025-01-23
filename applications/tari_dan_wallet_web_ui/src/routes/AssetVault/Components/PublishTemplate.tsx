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
import { useAccountsList } from "../../../api/hooks/useAccounts";
import { useTheme } from "@mui/material/styles";
import useAccountStore from "../../../store/accountStore";
import Select from "@mui/material/Select";
import { SelectChangeEvent } from "@mui/material/Select/Select";
import MenuItem from "@mui/material/MenuItem";
import { useFilePicker } from "use-file-picker";
import { ResourceAddress, ResourceType, substateIdToString } from "@tari-project/typescript-bindings";
import InputLabel from "@mui/material/InputLabel";
import { usePublishTemplate } from "../../../api/hooks/useTransactions";
import { Input } from "@mui/material";
import { FileAmountLimitValidator, FileSizeValidator, FileTypeValidator } from "use-file-picker/validators";
import { FileContent } from "use-file-picker/types";
import { base64FromArrayBuffer } from "../../../utils/helpers";

export default function PublishTemplate() {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button variant="outlined" onClick={() => setOpen(true)}>
        Publish Template
      </Button>
      <PublishTemplateDialog
        open={open}
        handleClose={() => setOpen(false)}
        onSendComplete={() => setOpen(false)}
        resource_type="Confidential"
      />
    </>
  );
}

export interface DialogProps {
  open: boolean;
  resource_address?: ResourceAddress;
  resource_type?: ResourceType;
  onSendComplete?: () => void;
  handleClose: () => void;
}

interface FormState {
  binary: ArrayBuffer | null;
  file: FileContent<ArrayBuffer> | null;
  account: string | null;
  maxFee: number | null;
}

function PublishTemplateDialog(props: DialogProps) {
  const INITIAL_VALUES = {
    binary: null,
    file: null,
    account: null,
    maxFee: null,
  };
  const [disabled, setDisabled] = useState(false);
  const [formState, setFormState] = useState<FormState>(INITIAL_VALUES);
  const [validity, setValidity] = useState<object>({
    file: false,
    account: false,
    maxFee: true,
  });
  const [allValid, setAllValid] = useState(false);

  const { account, setPopup } = useAccountStore();

  const theme = useTheme();

  let { data: accountsResp } = useAccountsList(0, 1000);
  let accounts = accountsResp?.accounts;

  const { mutateAsync: publishTemplate } = usePublishTemplate();

  function setFormValue(e: React.ChangeEvent<HTMLInputElement>) {
    setFormState({
      ...formState,
      [e.target.name]: e.target.value,
    });
    if (validity[e.target.name as keyof object] !== undefined) {
      setValidity({
        ...validity,
        [e.target.name]: e.target.validity.valid,
      });
    }
  }

  function setSelectFormValue(e: SelectChangeEvent<unknown>) {
    setFormState({
      ...formState,
      [e.target.name]: e.target.value,
    });
  }

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!account) {
      return;
    }
    setDisabled(true);
    const isDryRun = !formState.maxFee;
    publishTemplate({
      fee_account: { ComponentAddress: substateIdToString(account.address) },
      binary: base64FromArrayBuffer(formState.binary!),
      max_fee: isDryRun ? 1_000_000 : Number(formState.maxFee) || 0,
      detect_inputs: true,
      dry_run: isDryRun,
    })
      .then((resp) => {
        if (isDryRun) {
          setFormState({ ...formState, maxFee: resp.dry_run_fee! });
        } else {
          setFormState(INITIAL_VALUES);
          props.onSendComplete?.();
          setPopup({ title: "Publish template transaction submitted", error: false });
        }
      })
      .catch((e) => {
        setPopup({ title: "Publish failed", error: true, message: e.message });
      })
      .finally(() => {
        setDisabled(false);
      });
  };

  const handleClose = () => {
    props.handleClose?.();
  };

  useEffect(() => {
    setAllValid(Object.values(validity).every((v) => v));
  }, [validity]);

  const {
    openFilePicker,
    errors: fpErrors,
    loading: fpLoading,
  } = useFilePicker({
    multiple: false,
    readAs: "ArrayBuffer",
    validators: [
      new FileAmountLimitValidator({ max: 1 }),
      new FileTypeValidator(["wasm"]),
      new FileSizeValidator({ maxFileSize: 5 * 1024 * 1024 }),
    ],
    onFilesSuccessfullySelected: ({ filesContent }) => {
      setFormState({
        ...formState,
        binary: filesContent[0].content,
        file: filesContent[0],
      });
      setValidity({
        ...validity,
        file: true,
      });
    },
    onFilesRejected: () => {
      setValidity({
        ...validity,
        file: false,
      });
    },
  });

  useEffect(() => {
    let account = accounts?.find((a) => a.account.is_default)?.account.name || null;
    if (account) {
      setFormState({ ...INITIAL_VALUES, account });
      setValidity({ ...validity, account: true });
    }
  }, [accounts]);

  return (
    <Dialog open={props.open} onClose={handleClose}>
      <DialogTitle>Send {props.resource_address}</DialogTitle>
      <DialogContent className="dialog-content">
        <Form onSubmit={onSubmit} className="flex-container-vertical" style={{ paddingTop: theme.spacing(1) }}>
          {accounts && (
            <>
              <InputLabel id="select-account">Account</InputLabel>
              <Select
                id="select-account"
                name="account"
                disabled={disabled}
                displayEmpty
                value={formState.account || accounts.find((a) => a.account.is_default) || ""}
                onChange={setSelectFormValue}
                variant="outlined"
              >
                {accounts.map((account, i) => (
                  <MenuItem key={i} value={account.account.name!}>
                    {account.account.name} {account.account.is_default ? "(default)" : ""}
                  </MenuItem>
                ))}
              </Select>
            </>
          )}
          <Box
            className="flex-container"
            style={{
              justifyContent: "flex-start",
            }}
          >
            <Button
              variant="outlined"
              onClick={(e) => {
                e.preventDefault();
                openFilePicker();
              }}
              disabled={disabled || fpLoading}
            >
              Select WASM
            </Button>
            {formState.file && (
              <p style={{ color: "blue" }}>
                {formState.file.name} {formState.binary?.byteLength} bytes
              </p>
            )}
            {fpErrors[0] && <p style={{ color: "red" }}>{fpErrors[0].name}</p>}
          </Box>
          <TextField
            name="maxFee"
            label="Fee"
            type="number"
            value={formState.maxFee}
            placeholder="Enter max fee"
            onChange={setFormValue}
            disabled={disabled}
            style={{ flexGrow: 1 }}
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
            <Button variant="contained" type="submit" disabled={disabled || fpLoading || !allValid}>
              {formState.maxFee ? "Publish" : "Estimate fee"}
            </Button>
          </Box>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
