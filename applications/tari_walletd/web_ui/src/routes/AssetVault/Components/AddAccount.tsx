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

import { FormEvent, useState } from "react";
import { Form } from "react-router-dom";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import { CircularProgress } from "@mui/material";
import { useAccountsCreate } from "@api/hooks/useAccounts";
import useAccountStore from "@store/accountStore";
import type { AccountsCreateResponse } from "@tari-project/typescript-bindings";
import { useTheme } from "@mui/material/styles";
import queryClient from "@api/queryClient";
import { Stack, Fade, Typography } from "@mui/material";
import CheckCircleRoundedIcon from "@mui/icons-material/CheckCircleRounded";
import PopupTitle from "@/components/PopupTitle";

function AddAccount({ open, setOpen }: { open: boolean; setOpen: React.Dispatch<React.SetStateAction<boolean>> }) {
  const [accountFormState, setAccountFormState] = useState({
    accountName: "",
  });
  const { mutateAsync: mutateAddAccount, isPending, error, isSuccess, reset } = useAccountsCreate();
  const theme = useTheme();
  const setAccount = useAccountStore((state) => state.setAccount);
  const setOotleAddress = useAccountStore((state) => state.setOotleAddress);

  const handleClose = () => {
    setAccountFormState({ accountName: "" });
    reset();
    setOpen(false);
  };

  const onSubmitAddAccount = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const newAccount: AccountsCreateResponse = await mutateAddAccount({ accountName: accountFormState.accountName });
      setAccount(newAccount.account);
      setOotleAddress(newAccount.address);
      await queryClient.invalidateQueries({ queryKey: ["accounts"] });
      setTimeout(() => {
        handleClose();
      }, 3000);
    } catch (error) {
      console.error("Failed to create account:", error);
    }
  };

  const onAccountChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setAccountFormState({
      ...accountFormState,
      [e.target.name]: e.target.value,
    });
  };

  const getErrorMessage = (error: any): string => {
    if (!error) return "";
    const message = error.message || "";
    const invalidRequestMatch = message.match(/Invalid request:\s*(.+)/);
    if (invalidRequestMatch) {
      return invalidRequestMatch[1];
    }
    return message || "Failed to create account. Please try again.";
  };

  return (
    <Dialog open={open} onClose={handleClose}>
      <PopupTitle title="Add Account" onClose={handleClose} />
      {!isSuccess ? (
        <DialogContent className="dialog-content">
          <Form
            onSubmit={onSubmitAddAccount}
            className="flex-container-vertical"
            style={{ paddingTop: theme.spacing(1) }}
          >
            <TextField
              name="accountName"
              label="Account Name"
              value={accountFormState.accountName}
              onChange={onAccountChange}
              style={{ flexGrow: 1 }}
              disabled={isPending}
              autoFocus
              error={!!error}
              helperText={error ? getErrorMessage(error) : ""}
            />

            <Stack
              direction="row"
              justifyContent="flex-end"
              alignItems="center"
              spacing={2}
              sx={{ marginTop: theme.spacing(2), width: "100%" }}
            >
              <Button variant="outlined" onClick={handleClose} disabled={isPending}>
                Cancel
              </Button>
              <Button
                variant="contained"
                type="submit"
                disabled={isPending || !accountFormState.accountName.trim()}
                startIcon={isPending ? <CircularProgress size={16} /> : null}
              >
                {isPending ? "Creating..." : "Add Account"}
              </Button>
            </Stack>
          </Form>
        </DialogContent>
      ) : (
        <DialogContent className="dialog-content">
          <Stack
            direction="column"
            alignItems="center"
            justifyContent="center"
            spacing={1}
            sx={{
              minHeight: "250px",
            }}
          >
            <Fade in>
              <CheckCircleRoundedIcon sx={{ fontSize: 60, color: "success.main" }} />
            </Fade>
            <Typography variant="h5">Account created successfully!</Typography>
          </Stack>
        </DialogContent>
      )}
    </Dialog>
  );
}

export default AddAccount;
