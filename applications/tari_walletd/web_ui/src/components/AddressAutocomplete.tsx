// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useAccountsList } from "@api/hooks/useAccounts";
import { useAddressBookList } from "@api/hooks/useAddressBook";
import Autocomplete from "@mui/material/Autocomplete";
import TextField from "@mui/material/TextField";
import { shortenString } from "@utils/helpers";
import { ReactNode, useMemo } from "react";

interface AddressAutocompleteOption {
  label: string;
  value: string;
}

interface AddressAutocompleteProps {
  value: string;
  onChange: (address: string) => void;
  disabled?: boolean;
  required?: boolean;
  name?: string;
  label?: string;
  placeholder?: string;
  error?: boolean;
  helperText?: ReactNode;
}

export default function AddressAutocomplete({
  value,
  onChange,
  disabled,
  required,
  name = "address",
  label = "To Address",
  placeholder = "otl_loc_... or select from address book",
  error,
  helperText,
}: AddressAutocompleteProps) {
  const { data: addressBookData } = useAddressBookList();
  const { data: accounts } = useAccountsList(0, 100);

  const options = useMemo<AddressAutocompleteOption[]>(
    () =>
      (accounts?.accounts ?? [])
        .map((acc) => ({
          label: `${acc.account.name || ""} (${shortenString(acc.address)})`,
          value: acc.address,
        }))
        .concat(
          (addressBookData?.entries ?? []).map((e) => ({
            label: `${e.name} (${shortenString(e.address)})`,
            value: e.address,
          })),
        ),
    [accounts, addressBookData],
  );

  return (
    <Autocomplete
      freeSolo
      options={options}
      inputValue={value}
      onInputChange={(_e, newValue, reason) => {
        if (reason === "input" || reason === "clear") {
          onChange(newValue);
        }
      }}
      onChange={(_e, option) => {
        if (option && typeof option !== "string") {
          onChange(option.value);
        }
      }}
      disabled={disabled}
      renderInput={(params) => (
        <TextField
          {...params}
          name={name}
          label={label}
          required={required}
          placeholder={placeholder}
          error={error}
          helperText={helperText}
        />
      )}
    />
  );
}
