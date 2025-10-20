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
import { Form, Link } from "react-router-dom";
import AddIcon from "@mui/icons-material/Add";
import Button from "@mui/material/Button/Button";
import Fade from "@mui/material/Fade";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TextField from "@mui/material/TextField/TextField";
import { ChevronRight } from "@mui/icons-material";
import IconButton from "@mui/material/IconButton";
import { BoxHeading2, DataTableCell } from "@components/StyledComponents";
import { useAccountsCreate, useAccountsList } from "@api/hooks/useAccounts";
import FetchStatusCheck from "@components/FetchStatusCheck";
import queryClient from "@api/queryClient";
import { AccountInfo, substateIdToString, shortenSubstateId } from "@tari-project/typescript-bindings";
import CopyAddress from "@components/CopyAddress";

function Account(account: AccountInfo) {
  const {
    account: { name, component_address },
    address,
  } = account;
  return (
    <TableRow>
      <DataTableCell>
        <Link
          to={`/accounts/${substateIdToString(component_address)}`}
          style={{
            textDecoration: "none",
            color: "inherit",
          }}
        >
          {name || shortenSubstateId(component_address)}
        </Link>
      </DataTableCell>
      <DataTableCell>
        <CopyAddress address={substateIdToString(component_address)} />
      </DataTableCell>
      <DataTableCell>
        {account.account.owner_key_id && "Derived" in account.account.owner_key_id
          ? account.account.owner_key_id.Derived.index.toString()
          : "imported"}
      </DataTableCell>
      <DataTableCell>
        <CopyAddress address={address} />
      </DataTableCell>
      <DataTableCell>
        <IconButton component={Link} to={`/accounts/${substateIdToString(component_address)}`}>
          <ChevronRight />
        </IconButton>
      </DataTableCell>
    </TableRow>
  );
}

function Accounts() {
  const [showAccountDialog, setShowAddAccountDialog] = useState(false);
  const [accountFormState, setAccountFormState] = useState({
    accountName: "",
    signingKeyIndex: "",
    fee: "",
  });
  const {
    data: dataAccountsList,
    isLoading: isLoadingAccountsList,
    isError: isErrorAccountsList,
    error: errorAccountsList,
  } = useAccountsList(0, 10);

  const { mutateAsync: mutateAddAccount } = useAccountsCreate();

  const showAddAccountDialog = (setElseToggle: boolean = !showAccountDialog) => {
    setShowAddAccountDialog(setElseToggle);
    setAccountFormState({
      accountName: "",
      signingKeyIndex: "",
      fee: "",
    });
  };

  const onSubmitAddAccount = async () => {
    await mutateAddAccount(
      {
        accountName: accountFormState.accountName,
      },
      {
        onSettled: () => {
          setAccountFormState({
            accountName: "",
            signingKeyIndex: "",
            fee: "",
          });
          setShowAddAccountDialog(false);
          queryClient.invalidateQueries({ queryKey: ["accounts"] });
        },
      },
    );
  };

  const onAccountChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    e.preventDefault();
    setAccountFormState({
      ...accountFormState,
      [e.target.name]: e.target.value,
    });
  };

  return (
    <>
      <BoxHeading2
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "0.5rem",
        }}
      >
        {showAccountDialog && (
          <Fade in={showAccountDialog}>
            <Form onSubmit={onSubmitAddAccount} className="flex-container">
              <TextField
                name="accountName"
                label="Account Name"
                value={accountFormState.accountName}
                onChange={onAccountChange}
                style={{ flexGrow: 1 }}
              />
              <Button variant="contained" type="submit">
                Add Account
              </Button>
              <Button variant="outlined" onClick={() => showAddAccountDialog(false)}>
                Cancel
              </Button>
            </Form>
          </Fade>
        )}
        {!showAccountDialog && (
          <Fade in={!showAccountDialog}>
            <div className="flex-container">
              <Button variant="outlined" startIcon={<AddIcon />} onClick={() => showAddAccountDialog()}>
                Add Account
              </Button>
            </div>
          </Fade>
        )}
      </BoxHeading2>
      <FetchStatusCheck
        isLoading={isLoadingAccountsList}
        isError={isErrorAccountsList}
        errorMessage={errorAccountsList?.message || "Error fetching data"}
      >
        <Fade in={!isLoadingAccountsList && !isErrorAccountsList}>
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>Name</TableCell>
                  <TableCell>Component</TableCell>
                  <TableCell>Key index</TableCell>
                  <TableCell>Address</TableCell>
                  <TableCell>Details</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {dataAccountsList &&
                  dataAccountsList.accounts.map((account: AccountInfo, index: number) => Account(account, index))}
              </TableBody>
            </Table>
          </TableContainer>
        </Fade>
      </FetchStatusCheck>
    </>
  );
}

export default Accounts;
