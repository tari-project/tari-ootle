//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import PageHeading from "@/components/PageHeading";
import { useAccountsGetDefault } from "@api/hooks/useAccounts";
import FetchStatusCheck from "@components/FetchStatusCheck";
import PageHeader from "@components/PageHeader";
import { StyledPaper } from "@components/StyledComponents";
import Grid from "@mui/material/Grid";
import ConfidentialBalanceDisplay from "@routes/AssetVault/Components/ConfidentialBalanceDisplay";
import ConfidentialOutputList from "@routes/ConfidentialOutputList/ConfidentialOutputList";
import useAccountStore, { setAccount, setOotleAddress } from "@store/accountStore";
import { useEffect } from "react";

function ConfidentialOutputListPage() {
  const account = useAccountStore((state) => state.account);
  const { data: defaultAccount, isLoading, isError, error } = useAccountsGetDefault();

  useEffect(() => {
    if (!isError && defaultAccount && !account) {
      setAccount(defaultAccount.account);
      setOotleAddress(defaultAccount.address);
    }
  }, [defaultAccount, isError, account]);

  return (
    <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error loading account"}>
      {account ? (
        <>
          <PageHeader title="Confidential Outputs" balanceComponent={<ConfidentialBalanceDisplay />} />

          <Grid size={12}>
            <StyledPaper>
              <ConfidentialOutputList account={account} />
            </StyledPaper>
          </Grid>
        </>
      ) : (
        <Grid size={12}>
          <PageHeading>No Account Available</PageHeading>
        </Grid>
      )}
    </FetchStatusCheck>
  );
}

export default ConfidentialOutputListPage;
