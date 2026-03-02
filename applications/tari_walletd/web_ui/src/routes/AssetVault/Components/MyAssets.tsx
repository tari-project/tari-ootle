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

import { refreshAccountsBalances, useAccountsGetDefault } from "@api/hooks/useAccounts";
import queryClient from "@api/queryClient";
import Loading from "@components/Loading";
import PageHeader from "@components/PageHeader";
import { InnerHeading, StyledPaper } from "@components/StyledComponents";
import { Refresh } from "@mui/icons-material";
import Grid from "@mui/material/Grid";
import IconButton from "@mui/material/IconButton";
import { useTheme } from "@mui/material/styles";
import Transactions from "@routes/Transactions/Transactions";
import useAccountStore from "@store/accountStore";
import { substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useEffect } from "react";
import { Navigate } from "react-router";
import AccountDetails from "./AccountDetails";
import ActionMenu from "./ActionMenu";
import Assets from "./Assets";

function MyAssets() {
  const theme = useTheme();

  const { account, setAccount, setOotleAddress } = useAccountStore();
  const { data: defaultAccount, error, refetch } = useAccountsGetDefault(false);
  const refreshBalances = refreshAccountsBalances();

  useEffect(() => {
    refetch();

    if (!account && defaultAccount) {
      setAccount(defaultAccount.account);
      setOotleAddress(defaultAccount.address);
    }
  }, [account, defaultAccount]);

  // Default account not found. Redirect to onboarding
  if (error && (error.cause as any)?.code === 404) {
    console.error(error);
    // authStore.clearToken();
    return <Navigate replace to={"/onboarding"} />;
  }

  if (!account) {
    return <Loading />;
  }
  const handleRefreshClicked = () => {
    refreshBalances.mutate(substateIdToString(account.component_address));
    queryClient.invalidateQueries({
      predicate: (query) => {
        const key = query.queryKey[0];
        return typeof key === "string" && (key === "nfts" || key === "list_nfts" || key === "nfts_list");
      },
    });
  };

  return (
    <>
      <PageHeader title="My Assets" rightComponent={<ActionMenu />} />

      <Grid size={12}>
        <StyledPaper>
          <InnerHeading>Account Details</InnerHeading>
          <AccountDetails />
        </StyledPaper>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <InnerHeading>
            Assets
            <IconButton
              title="Refresh all accounts"
              color="primary"
              disabled={refreshBalances.isPending}
              onClick={handleRefreshClicked}
              size="small"
              sx={{
                marginLeft: theme.spacing(1),
              }}
            >
              <Refresh />
            </IconButton>
          </InnerHeading>
          <Assets account={account} />
        </StyledPaper>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <InnerHeading>Transactions</InnerHeading>
          <Transactions account={account} />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default MyAssets;
