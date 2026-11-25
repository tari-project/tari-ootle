// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { shortenString } from "@utils/helpers";

export interface AddressAutocompleteOption {
  key: string;
  label: string;
  value: string;
}

export interface AccountOptionInput {
  account: { name?: string | null };
  address: string;
}

export interface AddressBookOptionInput {
  name: string;
  address: string;
}

export function formatAddressOptionLabel(name: string | null | undefined, address: string): string {
  const trimmedName = name?.trim();
  const shortened = shortenString(address);
  return trimmedName ? `${trimmedName} (${shortened})` : shortened;
}

export function buildAddressOptions(
  accounts: readonly AccountOptionInput[] | undefined,
  addressBookEntries: readonly AddressBookOptionInput[] | undefined,
): AddressAutocompleteOption[] {
  const accountOptions: AddressAutocompleteOption[] = (accounts ?? []).map((acc, index) => ({
    key: `account:${acc.address}:${index}`,
    label: formatAddressOptionLabel(acc.account.name, acc.address),
    value: acc.address,
  }));

  const contactOptions: AddressAutocompleteOption[] = (addressBookEntries ?? []).map((entry, index) => ({
    key: `contact:${entry.name}:${entry.address}:${index}`,
    label: formatAddressOptionLabel(entry.name, entry.address),
    value: entry.address,
  }));

  return accountOptions.concat(contactOptions);
}

export function filterAddressOptions(
  options: readonly AddressAutocompleteOption[],
  inputValue: string,
): AddressAutocompleteOption[] {
  const query = inputValue.trim().toLowerCase();
  if (!query) {
    return [...options];
  }
  return options.filter(
    (option) => option.label.toLowerCase().includes(query) || option.value.toLowerCase().includes(query),
  );
}

export function resolveSelectedAddressOption(
  options: readonly AddressAutocompleteOption[],
  value: string,
): AddressAutocompleteOption | string | null {
  if (!value) {
    return null;
  }
  return options.find((option) => option.value === value) ?? value;
}
