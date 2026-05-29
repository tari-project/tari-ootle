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

import { useAccountsList } from "@api/hooks/useAccounts";
import PageHeading from "@components/PageHeading";
import { StyledPaper } from "@components/StyledComponents";
import FormControl from "@mui/material/FormControl";
import Grid from "@mui/material/Grid";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select, { SelectChangeEvent } from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import { Account } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import Transactions from "./Transactions";

// Empty value represents the "All accounts" option (no account filter).
const ALL_ACCOUNTS = "";

function TransactionsLayout() {
  const { data } = useAccountsList(0, 100);
  const [selected, setSelected] = useState<string>(ALL_ACCOUNTS);

  const accounts = data?.accounts ?? [];
  const selectedAccount: Account | null =
    accounts.find((a) => a.account.component_address === selected)?.account ?? null;

  const onChange = (e: SelectChangeEvent) => setSelected(e.target.value);

  return (
    <>
      <Grid size={12}>
        <PageHeading>Transactions</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <Stack direction="row" justifyContent="flex-end" sx={{ mb: 2 }}>
            <FormControl size="small" style={{ minWidth: 240 }}>
              <InputLabel id="transactions-account-filter">Account</InputLabel>
              <Select
                labelId="transactions-account-filter"
                label="Account"
                value={selected}
                onChange={onChange}
              >
                <MenuItem value={ALL_ACCOUNTS}>All accounts</MenuItem>
                {accounts.map(({ account }) => (
                  <MenuItem key={account.component_address} value={account.component_address}>
                    {account.name || account.component_address}
                  </MenuItem>
                ))}
              </Select>
            </FormControl>
          </Stack>
          <Transactions account={selectedAccount} />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default TransactionsLayout;
