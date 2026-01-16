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
import PageHeading from "@components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper, InnerHeading } from "@components/StyledComponents";
import TableContainer from "@mui/material/TableContainer";
import Table from "@mui/material/Table";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TableCell from "@mui/material/TableCell";
import TableBody from "@mui/material/TableBody";
import { useAccountsGetBalances, useAccountsGet } from "../../services/api/hooks/useAccounts";
import AccountName from "@components/AccountName";
import { useNFTsList } from "@api/hooks/useNfts";
import { ApiError } from "@api/helpers/types";
import { DataTableCell } from "@components/StyledComponents";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { BalanceEntry, decodeOotleAddressOrNull, substateIdToString } from "@tari-project/ootle-ts-bindings";
import NftList from "@routes/AssetVault/NFTs/NFTList";
import CopyAddress from "@components/CopyAddress";
import { Form, useParams } from "react-router-dom";
import { accountsAssociateStealthResource } from "@utils/json_rpc";
import Loading from "@components/Loading";
import { IoAdd } from "react-icons/io5";
import { Box, Fade, TextField, Button } from "@mui/material";
import { handleChangePage, handleChangeRowsPerPage } from "@utils/helpers";
import { formatCurrency } from "@utils/helpers";

function BalanceRow(props: BalanceEntry) {
  return (
    <TableRow key={props.resource_address}>
      <DataTableCell>
        <CopyAddress address={props.resource_address} display={props.token_symbol || props.resource_address} />
      </DataTableCell>
      <DataTableCell>{props.resource_type}</DataTableCell>
      <DataTableCell>{formatCurrency(props.balance)}</DataTableCell>
      <DataTableCell>{formatCurrency(props.confidential_balance)}</DataTableCell>
    </TableRow>
  );
}

function AccountDetailsLayout() {
  const { id: accountAddr } = useParams();
  const [showAddStealth, setShowAddStealth] = useState(false);
  const [stealthResource, setStealthResource] = useState({ newResourceAddress: "" });
  const [nftPage, setNftPage] = useState(0);
  const [nftRowsPerPage, setNftRowsPerPage] = useState(12);

  const {
    data: balancesData,
    isLoading: balancesIsLoading,
    isError: balancesIsError,
    error: balancesError,
  } = useAccountsGetBalances(substateIdToString(accountAddr!));

  const {
    data: accountsData,
    isLoading: accountsIsLoading,
    isError: accountsIsError,
    error: accountsError,
  } = useAccountsGet(accountAddr!);

  const offset = nftPage * nftRowsPerPage;
  const {
    data: nftsListData,
    isLoading: nftsListIsFetching,
    isError: nftsListIsError,
    error: nftsListError,
  } = useNFTsList(substateIdToString(accountAddr!), offset, nftRowsPerPage);

  const currentNfts = nftsListData?.nfts || [];
  const hasMore = currentNfts.length === nftRowsPerPage;
  const estimatedTotal = hasMore ? (nftPage + 1) * nftRowsPerPage + 1 : nftPage * nftRowsPerPage + currentNfts.length;

  const onStealthResourceChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setStealthResource({ ...stealthResource, [event.target.name]: event.target.value });
  };

  const onSubmitAddStealthResource = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (stealthResource.newResourceAddress) {
      accountsAssociateStealthResource({
        account: { ComponentAddress: accountAddr! },
        resource_address: stealthResource.newResourceAddress,
      })
        .then(() => {
          setStealthResource({ newResourceAddress: "" });
          setShowAddStealth(false);
        })
        .catch((error) => {
          console.error("Error adding stealth resource:", error);
        });
    }
  };

  if (balancesIsLoading || accountsIsLoading || nftsListIsFetching) {
    return <Loading />;
  }

  return (
    <>
      {accountsIsError && <div>Error loading account: {accountsError?.message}</div>}
      {balancesIsError && <div>Error loading balances: {balancesError?.message}</div>}
      {nftsListIsError && <div>Error loading NFTs: {nftsListError?.message}</div>}

      <Grid item xs={12} md={12} lg={12}>
        <PageHeading>Account Details</PageHeading>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <Button
            variant="outlined"
            startIcon={<IoAdd />}
            title="Associates a stealth resource with this account. This will enable periodic scans for this resource"
            onClick={() => setShowAddStealth(!showAddStealth)}
          >
            Add Stealth Resource
          </Button>
          <Box mt={2} mb={2}>
            <Fade in={showAddStealth}>
              <Form onSubmit={onSubmitAddStealthResource} className="flex-container">
                <TextField
                  name="newResourceAddress"
                  label="Resource Address"
                  value={stealthResource.newResourceAddress}
                  onChange={onStealthResourceChange}
                  style={{ flexGrow: 1 }}
                />
                <Button variant="contained" type="submit">
                  Add Stealth Resource
                </Button>
                <Button variant="outlined" onClick={() => setShowAddStealth(false)}>
                  Cancel
                </Button>
              </Form>
            </Fade>
          </Box>
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>Name</TableCell>
                  <TableCell>Component</TableCell>
                  <TableCell>Address</TableCell>
                  <TableCell>Public Key</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                <TableRow>
                  <DataTableCell>
                    <AccountName accountAddress={accountAddr!} currentName={accountsData?.account?.name} />
                  </DataTableCell>
                  <DataTableCell>
                    <CopyAddress address={substateIdToString(accountsData?.account.component_address)} />
                  </DataTableCell>
                  <DataTableCell>
                    {" "}
                    {accountsData?.address && <CopyAddress address={accountsData?.address!} />}{" "}
                  </DataTableCell>
                  <DataTableCell>
                    {" "}
                    {accountsData?.address && (
                      <CopyAddress
                        address={decodeOotleAddressOrNull(accountsData?.address!)?.accountPublicKey || "<decode error>"}
                      />
                    )}{" "}
                  </DataTableCell>
                </TableRow>
              </TableBody>
            </Table>
          </TableContainer>
        </StyledPaper>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <InnerHeading>Balances</InnerHeading>
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
          <InnerHeading>Account NFTs</InnerHeading>
          <NftList
            nftsListIsError={nftsListIsError}
            nftsListIsFetching={nftsListIsFetching}
            nftsListError={nftsListError as ApiError | null}
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
