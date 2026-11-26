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
import useAccountStore, { setAccount, setOotleAddress } from "@store/accountStore";
import { AccountInfo, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import { IoAdd } from "react-icons/io5";
import Dialog from "./AddAccount";
import { buildAccountSelectOptions, resolveSelectedAccountValue } from "./selectAccountHelpers";

function SelectAccount() {
  const account = useAccountStore((state) => state.account);
  const { data: dataAccountsList } = useAccountsList(0, 100);
  const [dialogOpen, setDialogOpen] = useState(false);
  const theme = useTheme();

  const options = buildAccountSelectOptions(dataAccountsList?.accounts, account);
  const selectedValue = resolveSelectedAccountValue(options, account);

  const handleChange = (event: SelectChangeEvent) => {
    const nextValue = event.target.value as string;
    if (nextValue === "addAccount") {
      setDialogOpen(true);
      return;
    }
    const selectedAccount = dataAccountsList?.accounts.find(
      (info: AccountInfo) => substateIdToString(info.account.component_address) === nextValue,
    );
    if (selectedAccount) {
      setAccount(selectedAccount.account);
      setOotleAddress(selectedAccount.address);
    }
  };

  return (
    <Box sx={{ minWidth: 250 }}>
      <Dialog open={dialogOpen} setOpen={setDialogOpen} />
      <FormControl fullWidth>
        <InputLabel id="account-select-label">Account</InputLabel>
        <Select
          labelId="account-select-label"
          id="account-select"
          size="small"
          value={selectedValue}
          label="Account"
          onChange={handleChange}
        >
          {options.map((opt) => (
            <MenuItem key={opt.address} value={opt.address}>
              {opt.label}
            </MenuItem>
          ))}
          <Divider />
          <MenuItem value={"addAccount"}>
            <IoAdd style={{ marginRight: theme.spacing(1) }} />
            Add Account
          </MenuItem>
        </Select>
      </FormControl>
    </Box>
  );
}

export default SelectAccount;
