import { useAccountsList } from "@api/hooks/useAccounts";
import { type SelectChangeEvent } from "@mui/material";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import useAccountStore, { setAccount, setOotleAddress } from "@store/accountStore";
import { substateIdToString } from "@tari-project/ootle-ts-bindings";

export default function AccountPicker() {
  const { data } = useAccountsList(0, 10);
  const currentAccount = useAccountStore((s) => s.account);
  const defaultAccount = data?.accounts.find((a) => a.account.is_default)?.account;
  const account = currentAccount || defaultAccount;

  function onAccountChange(e: SelectChangeEvent) {
    const selected = data?.accounts.find((a) => substateIdToString(a.account.component_address) === e.target.value);
    if (selected) {
      setAccount(selected.account);
      setOotleAddress(selected.address);
    }
  }
  const options = data?.accounts.map(({ account }) => {
    const address = substateIdToString(account.component_address);
    return (
      <MenuItem key={`item_${address}`} value={address}>
        {account.name || address}
      </MenuItem>
    );
  });

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
          {options}
        </Select>
      </FormControl>
    </Stack>
  ) : null;
}
