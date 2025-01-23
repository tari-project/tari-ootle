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

import { useMutation, useQuery } from "@tanstack/react-query";
import {
  accountsClaimBurn,
  accountsConfidentialTransfer,
  accountsCreate,
  accountsCreateFreeTestCoins,
  accountsGet,
  accountsGetBalances,
  accountsGetDefault,
  accountsList,
  accountsTransfer,
  nftList,
  transactionsPublishTemplate,
} from "../../utils/json_rpc";
import { ApiError } from "../helpers/types";
import queryClient from "../queryClient";
import type {
  ComponentAccessRules,
  ConfidentialTransferInputSelection,
  ComponentAddressOrName,
} from "@tari-project/typescript-bindings";

//   Fees are passed as strings because Amount is tagged
export const useAccountsClaimBurn = (account: string, claimProof: string, fee: number) => {
  return useMutation(
    () =>
      accountsClaimBurn({
        account: { Name: account },
        claim_proof: claimProof,
        max_fee: fee,
        key_id: null,
      }),
    {
      onError: (error: ApiError) => {
        error;
      },
      onSettled: () => {
        queryClient.invalidateQueries(["accounts"]);
      },
    },
  );
};

export const useAccountsCreate = (
  accountName: string | null,
  customAccessRules: ComponentAccessRules | null,
  fee: number | null,
  is_default: boolean,
) => {
  return useMutation(
    async () => {
      return await accountsCreate({
        account_name: accountName,
        custom_access_rules: customAccessRules,
        max_fee: fee,
        is_default,
        key_id: null,
      });
    },
    {
      onError: (error: ApiError) => {
        error;
      },
      onSettled: () => {
        queryClient.invalidateQueries(["accounts"]);
      },
    },
  );
};

export interface TransferParams {
  account: ComponentAddressOrName | null;
  amount: number;
  resource_address: string;
  destination_public_key: string;
  max_fee: number | null;
  isConfidential: boolean;
  output_to_revealed: boolean;
  input_selection: ConfidentialTransferInputSelection;
  badge: string | null;
  dry_run: boolean;
}

export const useAccountsTransfer = (params: TransferParams) => {
  return useMutation(
    () => {
      let transferRequest = {
        account: params.account,
        amount: params.amount,
        resource_address: params.resource_address,
        destination_public_key: params.destination_public_key,
        max_fee: params.max_fee,
        proof_from_badge_resource: params.badge,
        input_selection: params.input_selection,
        output_to_revealed: params.output_to_revealed,
        dry_run: params.dry_run,
      };
      if (params.isConfidential) {
        return accountsConfidentialTransfer(transferRequest);
      } else {
        return accountsTransfer(transferRequest);
      }
    },
    {
      onError: (error: ApiError) => {
        error;
      },
      onSettled: () => {
        queryClient.invalidateQueries(["accounts"]);
      },
    },
  );
};

export const useAccountsCreateFreeTestCoins = () => {
  const createFreeTestCoins = async ({
    account,
    amount,
    fee,
  }: {
    account: ComponentAddressOrName;
    amount: number;
    fee: number | null;
  }) => {
    const result = await accountsCreateFreeTestCoins({
      account,
      amount,
      max_fee: fee,
      key_id: null,
    });
    return result;
  };

  return useMutation(createFreeTestCoins, {
    onError: (error: ApiError) => {
      console.error(error);
    },
    onSettled: () => {
      queryClient.invalidateQueries(["transactions"]);
      queryClient.invalidateQueries(["accounts_balances"]);
    },
  });
};

export const useAccountsList = (offset: number, limit: number) => {
  return useQuery({
    queryKey: ["accounts"],
    queryFn: () => accountsList({ offset, limit }),
    onError: (error: ApiError) => {
      error;
    },
  });
};

export const useAccountsGetBalances = (account: ComponentAddressOrName | null, refresh: boolean = false) => {
  return useQuery({
    queryKey: ["accounts_balances"],
    queryFn: () => accountsGetBalances({ account, refresh }),
    onError: (_error: ApiError) => {},
    refetchInterval: 5000,
  });
};

export const useAccountsGetDefault = () => {
  return useQuery({
    queryKey: ["accounts_get_default"],
    queryFn: () => accountsGetDefault({}),
    refetchInterval: false,
    notifyOnChangeProps: ["data", "error"],
    retryOnMount: false,
    retry: false,
    onError: (error: ApiError) => {
      error;
    },
  });
};
export const useAccountsGet = (account: ComponentAddressOrName) => {
  return useQuery({
    queryKey: ["accounts_get"],
    queryFn: () => accountsGet({ name_or_address: account }),
    onError: (error: ApiError) => {
      error;
    },
  });
};

export const useAccountNFTsList = (account: ComponentAddressOrName | null, offset: number, limit: number) => {
  return useQuery({
    queryKey: ["nfts_list"],
    queryFn: () => nftList({ account, offset, limit }),
    onError: (error: ApiError) => {
      error;
    },
  });
};
