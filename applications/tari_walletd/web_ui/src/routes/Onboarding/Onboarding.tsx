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
import { Form, Navigate } from "react-router-dom";
import TextField from "@mui/material/TextField/TextField";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Grid from "@mui/material/Grid";
import Typography from "@mui/material/Typography";
import { useTheme } from "@mui/material/styles";
import Loading from "@components/Loading";
import { refreshAccountsBalances, useAccountsCreate, useAccountsGetDefault } from "@api/hooks/useAccounts";
import useAccountStore from "@store/accountStore";
import useAuthStore from "@store/authStore";

function Onboarding() {
  console.log("Onboarding rendered");
  const { mutate, isPending } = useAccountsCreate();
  const { account, setAccount, setOotleAddress, setPopup } = useAccountStore();
  const theme = useTheme();
  const [accountFormState, setAccountFormState] = useState({
    accountName: "",
  });
  const { data: defaultAccount, isLoading, isError, error } = useAccountsGetDefault();
  const authStore = useAuthStore();

  useEffect(() => {
    if (defaultAccount) {
      setAccount(defaultAccount.account);
      setOotleAddress(defaultAccount.address);
    }
  }, [account, defaultAccount]);

  // Handle 401 errors by redirecting to onboarding and marking the user as logged out
  // TODO: figure out how to do this for every request
  if (error && (error.cause as any)?.status === 401) {
    console.error(error, "Not logged in or session expired");
    authStore.setLoggedIn(false);
    return <Navigate replace to={"/"} />;
  }

  if (isLoading) {
    return <Loading />;
  }

  if (defaultAccount) {
    return <Navigate replace to={"/"} />;
  }

  const handleCreateAccount = (e: FormEvent) => {
    e.preventDefault();
    mutate(
      {
        accountName: accountFormState.accountName,
      },
      {
        onSuccess: (resp) => {
          setAccount(resp.account);
          setOotleAddress(resp.address);
        },
        onError: (e) => {
          setPopup({ title: "Account create failed: " + e, error: true });
        },
      },
    );
  };

  const onAccountChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setAccountFormState({
      ...accountFormState,
      [e.target.name]: e.target.value,
    });
  };

  if (isPending) {
    return <Loading />;
  }

  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        <Box
          style={{
            display: "flex",
            justifyContent: "center",
            alignItems: "center",
            flexDirection: "column",
            width: "100%",
            height: "calc(100vh - 200px)",
            minHeight: 400,
            gap: theme.spacing(3),
          }}
        >
          <Box
            style={{
              display: "flex",
              justifyContent: "center",
              alignItems: "center",
              flexDirection: "column",
              gap: 0,
              maxWidth: 600,
            }}
          >
            <Typography
              variant="h3"
              style={{
                textAlign: "center",
              }}
            >
              Welcome to the Tari Asset Vault
            </Typography>
            <Typography
              variant="h5"
              style={{
                textAlign: "center",
              }}
            >
              Give your account a friendly name to get started
            </Typography>
            <Form
              onSubmit={handleCreateAccount}
              className="flex-container"
              style={{
                flexDirection: "column",
                marginTop: theme.spacing(3),
              }}
            >
              <TextField
                name="accountName"
                label="Account Name"
                value={accountFormState.accountName}
                onChange={onAccountChange}
                style={{ flexGrow: 1 }}
              />
              <Button variant="contained" type="submit">
                Create Account
              </Button>
            </Form>
          </Box>
        </Box>
      </Grid>
    </>
  );
}

export default Onboarding;
