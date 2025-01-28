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

import Grid from "@mui/material/Grid";
import Box from "@mui/material/Box";
import Divider from "@mui/material/Divider";
import Typography from "@mui/material/Typography";
import { useTheme } from "@mui/material/styles";
import { useEffect } from "react";
import { InnerHeading, StyledPaper } from "../../../Components/StyledComponents";
import {
  useAccountNFTsList,
  useAccountsGet,
  useAccountsGetBalances,
  useAccountsGetDefault,
} from "../../../api/hooks/useAccounts";
import useAccountStore from "../../../store/accountStore";
import Transactions from "../../Transactions/Transactions";
import AccountBalance from "./AccountBalance";
import AccountDetails from "./AccountDetails";
import ActionMenu from "./ActionMenu";
import Assets from "./Assets";
import SelectAccount from "./SelectAccount";
import FetchStatusCheck from "../../../Components/FetchStatusCheck";
import { substateIdToString } from "@tari-project/typescript-bindings";

function MyAssets() {
  const theme = useTheme();
  const { account } = useAccountStore();

  if (!account) {
    return <>Loading...</>;
  }

  // const { refetch: balancesRefetch, data } = useAccountsGetBalances({
  //   ComponentAddress: substateIdToString(account.address),
  // });
  // const { refetch: nftsListRefetch } = useAccountNFTsList(
  //   { ComponentAddress: substateIdToString(account.address) },
  //   0,
  //   10,
  // );

  // useEffect(() => {
  //   balancesRefetch();
  //   nftsListRefetch();
  // }, [accountId]);

  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        <Box
          className="flex-container"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            width: "100%",
          }}
        >
          <Typography
            variant="h4"
            style={{
              paddingBottom: theme.spacing(2),
            }}
          >
            My Assets
          </Typography>
          <ActionMenu />
        </Box>
        <Divider />
      </Grid>

      <Grid
        item
        xs={12}
        md={12}
        lg={12}
        style={{
          position: "sticky",
          top: 50,
          background: theme.palette.background.default,
          opacity: 0.9,
          zIndex: 1,
          paddingBottom: theme.spacing(1),
        }}
      >
        <Box
          className="flex-container"
          style={{
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <AccountBalance />
          <SelectAccount />
        </Box>
      </Grid>

      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <InnerHeading>Account Details</InnerHeading>
          <AccountDetails />
        </StyledPaper>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <InnerHeading>Assets</InnerHeading>
          <Assets account={account} />
        </StyledPaper>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <InnerHeading>Transactions</InnerHeading>
          <Transactions account={account} />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default MyAssets;
