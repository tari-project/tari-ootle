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

import { useState } from "react";
import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import TableContainer from "@mui/material/TableContainer";
import Table from "@mui/material/Table";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TableCell from "@mui/material/TableCell";
import TableBody from "@mui/material/TableBody";
import { useAccountsGetBalances, useAccountNFTsList } from "../../api/hooks/useAccounts";
import { DataTableCell } from "../../Components/StyledComponents";
import FetchStatusCheck from "../../Components/FetchStatusCheck";
import { BalanceEntry, substateIdToString } from "@tari-project/typescript-bindings";
import NFTList from "../AssetVault/NFTs/NFTList";
import CopyAddress from "../../Components/CopyAddress";
import useAccountStore from "../../store/accountStore";
import { handleChangePage, handleChangeRowsPerPage } from "../../utils/helpers";

function BalanceRow(props: BalanceEntry) {
  return (
    <TableRow key={props.resource_address}>
      <DataTableCell>
        <CopyAddress address={props.resource_address} display={props.token_symbol || props.resource_address} />
      </DataTableCell>
      <DataTableCell>{props.resource_type}</DataTableCell>
      <DataTableCell>{props.balance}</DataTableCell>
      <DataTableCell>{props.confidential_balance}</DataTableCell>
    </TableRow>
  );
}

function AccountDetailsLayout() {
  const { account, publicKey } = useAccountStore();
  const [nftPage, setNftPage] = useState(0);
  const [nftRowsPerPage, setNftRowsPerPage] = useState(12);

  if (!account) {
    return <>No Account</>;
  }

  const {
    data: balancesData,
    isLoading: balancesIsLoading,
    isError: balancesIsError,
    error: balancesError,
  } = useAccountsGetBalances(substateIdToString(account.address));

  // const {
  //   data: accountsData,
  //   isLoading: accountsIsLoading,
  //   isError: accountsIsError,
  //   error: accountsError,
  // } = useAccountsGet({ ComponentAddress: accountId });

  const {
    data: nftsListData,
    isLoading: nftsListIsFetching,
    isError: nftsListIsError,
    error: nftsListError,
  } = useAccountNFTsList(substateIdToString(account.address), nftPage * nftRowsPerPage, nftRowsPerPage);

  const currentNfts = nftsListData?.nfts || [];
  const hasMore = currentNfts.length === nftRowsPerPage;
  const estimatedTotal = hasMore ? (nftPage + 1) * nftRowsPerPage + 1 : nftPage * nftRowsPerPage + currentNfts.length;

  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        <PageHeading>Account Details</PageHeading>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>Name</TableCell>
                  <TableCell>Address</TableCell>
                  <TableCell>Public key</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                <TableRow>
                  <DataTableCell>{account.name}</DataTableCell>
                  <DataTableCell>
                    <CopyAddress address={substateIdToString(account.address)} />
                  </DataTableCell>
                  <DataTableCell>
                    <CopyAddress address={publicKey} />
                  </DataTableCell>
                </TableRow>
              </TableBody>
            </Table>
          </TableContainer>
        </StyledPaper>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          Balances
          <FetchStatusCheck
            isError={balancesIsError}
            errorMessage={balancesError?.message || "Error fetching data"}
            isLoading={balancesIsLoading}
          >
            <TableContainer>
              <Table>
                <TableHead>
                  <TableRow>
                    <TableCell>Resource</TableCell>
                    <TableCell>Resource Type</TableCell>
                    <TableCell>Revealed Balance</TableCell>
                    <TableCell>Confidential Balance</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>{balancesData?.balances.map((balance: BalanceEntry) => BalanceRow(balance))}</TableBody>
              </Table>
            </TableContainer>
          </FetchStatusCheck>
        </StyledPaper>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          Account NFTs
          <NFTList
            nftsListIsError={nftsListIsError}
            nftsListIsFetching={nftsListIsFetching}
            nftsListError={nftsListError}
            nftsListData={nftsListData}
            totalCount={estimatedTotal}
            page={nftPage}
            rowsPerPage={nftRowsPerPage}
            onPageChange={(event, newPage) => handleChangePage(event, newPage, setNftPage)}
            onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setNftRowsPerPage, setNftPage)}
          />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default AccountDetailsLayout;
