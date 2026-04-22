// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import type {
  AddressBookAddRequest,
  AddressBookDeleteRequest,
  AddressBookUpdateRequest,
} from "@tari-project/ootle-ts-bindings";
import { addressBookAdd, addressBookDelete, addressBookList, addressBookUpdate } from "@utils/json_rpc";

const ADDRESS_BOOK_QUERY_KEY = ["address_book"];

export const useAddressBookList = () => {
  return useQuery({
    queryKey: ADDRESS_BOOK_QUERY_KEY,
    queryFn: () => addressBookList(),
  });
};

export const useAddressBookAdd = () => {
  return useMutation({
    mutationFn: (params: AddressBookAddRequest) => addressBookAdd(params),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ADDRESS_BOOK_QUERY_KEY });
    },
  });
};

export const useAddressBookUpdate = () => {
  return useMutation({
    mutationFn: (params: AddressBookUpdateRequest) => addressBookUpdate(params),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ADDRESS_BOOK_QUERY_KEY });
    },
  });
};

export const useAddressBookDelete = () => {
  return useMutation({
    mutationFn: (params: AddressBookDeleteRequest) => addressBookDelete(params),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ADDRESS_BOOK_QUERY_KEY });
    },
  });
};
