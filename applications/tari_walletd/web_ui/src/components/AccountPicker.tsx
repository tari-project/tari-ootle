//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import { useAccountsList } from "@api/hooks/useAccounts";
import { type SelectChangeEvent } from "@mui/material";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import useAccountStore, { setAccount, setOotleAddress } from "@store/accountStore";
import { substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useEffect, useMemo } from "react";
import { buildAccountPickerOptions, resolveFallbackAccount } from "./accountPickerHelpers";

const ACCOUNTS_FETCH_LIMIT = 100;

// TODO - replace other file-specific account selectors with this component
export default function AccountPicker() {
  const { data } = useAccountsList(0, ACCOUNTS_FETCH_LIMIT);
  const currentAccount = useAccountStore((s) => s.account);
  const fallbackAccount = useMemo(() => resolveFallbackAccount(data?.accounts), [data?.accounts]);
  const account = currentAccount || fallbackAccount?.account;

  useEffect(() => {
    if (!currentAccount && fallbackAccount) {
      setAccount(fallbackAccount.account);
      setOotleAddress(fallbackAccount.address);
    }
  }, [currentAccount, fallbackAccount]);

  function onAccountChange(e: SelectChangeEvent) {
    const selected = data?.accounts.find((a) => substateIdToString(a.account.component_address) === e.target.value);
    if (selected) {
      setAccount(selected.account);
      setOotleAddress(selected.address);
    }
  }

  const options = useMemo(() => buildAccountPickerOptions(data?.accounts, account), [data?.accounts, account]);

  return account ? (
    <Stack alignItems="center" justifyContent="flex-end" direction="row" spacing={2}>
      <FormControl style={{ minWidth: "30%" }}>
        <InputLabel id="account">Account</InputLabel>
        <Select
          labelId="account"
          name="account"
          label="Account"
          value={substateIdToString(account.component_address)}
          onChange={onAccountChange}
        >
          {options.map(({ address, label }) => (
            <MenuItem key={`item_${address}`} value={address}>
              {label}
            </MenuItem>
          ))}
        </Select>
      </FormControl>
    </Stack>
  ) : null;
}
