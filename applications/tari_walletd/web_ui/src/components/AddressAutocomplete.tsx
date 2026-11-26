// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useAccountsList } from "@api/hooks/useAccounts";
import { useAddressBookList } from "@api/hooks/useAddressBook";
import Autocomplete from "@mui/material/Autocomplete";
import TextField from "@mui/material/TextField";
import { ReactNode, useMemo } from "react";
import {
  AddressAutocompleteOption,
  buildAddressOptions,
  filterAddressOptions,
  resolveSelectedAddressOption,
} from "./addressAutocompleteHelpers";

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
    () => buildAddressOptions(accounts?.accounts, addressBookData?.entries),
    [accounts, addressBookData],
  );

  const selectedValue = useMemo(() => resolveSelectedAddressOption(options, value), [options, value]);

  return (
    <Autocomplete<AddressAutocompleteOption, false, false, true>
      freeSolo
      options={options}
      value={selectedValue}
      inputValue={value}
      getOptionLabel={(option) => (typeof option === "string" ? option : option.label)}
      getOptionKey={(option) => (typeof option === "string" ? option : option.key)}
      isOptionEqualToValue={(option, val) => (typeof val === "string" ? option.value === val : option.key === val.key)}
      filterOptions={(opts, state) => filterAddressOptions(opts, state.inputValue)}
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
