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
import Box from "@mui/material/Box";
import Divider from "@mui/material/Divider";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select, { SelectChangeEvent } from "@mui/material/Select";
import { useTheme } from "@mui/material/styles";
import useAccountStore from "@store/accountStore";
import { AccountInfo, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import { IoAdd } from "react-icons/io5";
import Dialog from "./AddAccount";

function SelectAccount() {
  const account = useAccountStore((state) => state.account);
  const setAccount = useAccountStore((state) => state.setAccount);
  const setOotleAddress = useAccountStore((state) => state.setOotleAddress);
  const { data: dataAccountsList } = useAccountsList(0, 10);
  const [dialogOpen, setDialogOpen] = useState(false);
  const theme = useTheme();

  const handleChange = (event: SelectChangeEvent) => {
    const selectedValue = event.target.value as string;
    if (selectedValue !== "addAccount") {
      const account = dataAccountsList?.accounts.find(
        (info: AccountInfo) => substateIdToString(info.account.component_address) === selectedValue,
      );
      if (account) {
        setAccount(account.account);
        setOotleAddress(account.address);
        // setAccountName(event.target.value as string);
      }
    }
  };

  const handleAddAccount = () => {
    setDialogOpen(true);
  };
  return (
    <Box sx={{ minWidth: 250 }}>
      <Dialog open={dialogOpen} setOpen={setDialogOpen} />
      <FormControl fullWidth>
        <InputLabel id="account-select-label">Account</InputLabel>
        <Select
          labelId="account-select-label"
          id="account-select"
          value={
            account &&
            dataAccountsList?.accounts.some(
              (info: AccountInfo) => info.account.component_address === account?.component_address,
            )
              ? substateIdToString(account.component_address)
              : dataAccountsList?.accounts.length
                ? substateIdToString(dataAccountsList.accounts[0].account.component_address)
                : "addAccount"
          }
          label="Account"
          onChange={handleChange}
        >
          {dataAccountsList?.accounts.map((account: AccountInfo, i) => {
            return (
              <MenuItem key={i} value={substateIdToString(account.account.component_address)}>
                {account.account.name || "<No Name>"}
              </MenuItem>
            );
          })}
          <Divider />
          <MenuItem value={"addAccount"} onClick={handleAddAccount}>
            <IoAdd style={{ marginRight: theme.spacing(1) }} />
            Add Account
          </MenuItem>
        </Select>
      </FormControl>
    </Box>
  );
}

export default SelectAccount;
