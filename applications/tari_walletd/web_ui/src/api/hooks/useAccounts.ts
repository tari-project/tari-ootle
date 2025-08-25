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
  accountsStealthTransfer,
  accountsTransfer,
  mintFaucetNfts,
  nftList,
  validatorsGetFees,
} from "../../utils/json_rpc";
import { ApiError } from "../helpers/types";
import queryClient from "../queryClient";
import type {
  AccountOrKeyIndex,
  ClaimBurnProof,
  ComponentAddress,
  ComponentAddressOrName,
  ConfidentialTransferInputSelection,
  ResourceType,
} from "@tari-project/typescript-bindings";

const DEFAULT_MAX_FEE = 2000;

//   Fees are passed as strings because Amount is tagged
export const useAccountsClaimBurn = (account: string, claimProof: ClaimBurnProof, fee: number) => {
  return useMutation(
    () =>
      accountsClaimBurn({
        account: { Name: account },
        claim_proof: claimProof,
        max_fee: fee,
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

export type AccountsCreateMutate = {
  accountName?: string;
  isDefault?: boolean;
  keyId?: number | null;
};

export const useAccountsCreate = () => {
  return useMutation(
    async (req: AccountsCreateMutate) => {
      return await accountsCreate({
        account_name: req.accountName || "",
        is_default: req.isDefault || null,
        key_id: req.keyId || null,
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
  account: ComponentAddress;
  amount: number;
  resource_address: string;
  destination_public_key: string;
  max_fee: number | null;
  resourceType: ResourceType;
  output_to_revealed: boolean;
  input_selection: ConfidentialTransferInputSelection;
  badge: string | null;
  dry_run: boolean;
}

export const useAccountsTransfer = () => {
  return useMutation(
    (params: TransferParams) => {
      const account = { ComponentAddress: params.account };
      const max_fee = params.max_fee || DEFAULT_MAX_FEE;
      if (params.resourceType === "Confidential") {
        let transferRequest = {
          account,
          amount: params.amount,
          resource_address: params.resource_address,
          destination_public_key: params.destination_public_key,
          max_fee,
          proof_from_badge_resource: params.badge,
          input_selection: params.input_selection,
          output_to_revealed: params.output_to_revealed,
          dry_run: params.dry_run,
        };
        return accountsConfidentialTransfer(transferRequest);
      } else if (params.resourceType === "Stealth") {
        let transferRequest = {
          owner_account: account,
          input_selection: params.input_selection,
          resource_address: params.resource_address,
          destination_public_key: params.destination_public_key,
          max_fee,
          blinded_output_amount: params.output_to_revealed ? 0 : params.amount,
          revealed_output_amount: params.output_to_revealed ? params.amount : 0,
          dry_run: params.dry_run,
        };
        return accountsStealthTransfer(transferRequest);
      } else {
        // Fungible and NFTs
        let transferRequest = {
          account,
          amount: params.amount,
          resource_address: params.resource_address,
          destination_public_key: params.destination_public_key,
          max_fee,
          proof_from_badge_resource: params.badge,
          input_selection: params.input_selection,
          output_to_revealed: params.output_to_revealed,
          dry_run: params.dry_run,
        };
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
  }) =>
    accountsCreateFreeTestCoins({
      account,
      amount,
      max_fee: fee,
    });

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

export const useMintTestnetFaucetNfts = () => {
  const callApi = async ({
    account,
    numberToMint,
    mutableData,
    maxFee,
  }: {
    account: ComponentAddressOrName;
    numberToMint: number;
    mutableData: object;
    maxFee: number | null;
  }) =>
    mintFaucetNfts({
      account,
      mutable_data: mutableData,
      number_to_mint: BigInt(numberToMint),
      max_fee: maxFee,
    });

  return useMutation(callApi, {
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

export const useAccountsGetBalances = (account: ComponentAddress, refresh: boolean = false) => {
  return useQuery({
    queryKey: [`accounts_balances_${account}`],
    queryFn: () => accountsGetBalances({ account: { ComponentAddress: account }, refresh }),
    onError: (_error: ApiError) => {},
    refetchInterval: 5000,
    structuralSharing: (oldData, newData) => {
      if (!oldData || !newData) return newData;
      if (JSON.stringify(oldData) === JSON.stringify(newData)) {
        return oldData;
      }
      return newData;
    },
  });
};

export const refreshAccountsBalances = (account: ComponentAddress) => {
  return useMutation(() => accountsGetBalances({ account: { ComponentAddress: account }, refresh: true }), {
    onError: (error: ApiError) => {
      error;
    },
    onSettled: () => {
      queryClient.invalidateQueries(["accounts_balances_" + account]);
    },
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
    onError: (_error: ApiError) => {},
  });
};
export const useAccountsGet = (account: ComponentAddress) => {
  return useQuery({
    queryKey: ["accounts_get_" + account],
    queryFn: () => accountsGet({ name_or_address: { ComponentAddress: account } }),
    onError: (_error: ApiError) => {},
  });
};

export const useAccountNFTsList = (account: ComponentAddress, offset: number, limit: number) => {
  return useQuery({
    queryKey: ["nfts_list_" + account + "_" + offset + "_" + limit],
    queryFn: () => nftList({ account: { ComponentAddress: account }, offset, limit }),
    onError: (_error: ApiError) => {},
    structuralSharing: (oldData, newData) => {
      if (!oldData || !newData) return newData;
      if (JSON.stringify(oldData) === JSON.stringify(newData)) {
        return oldData;
      }
      return newData;
    },
  });
};


export const useValidatorFees = (accountOrKeyIndex: AccountOrKeyIndex, shardGroup = null) => {
  return useQuery({
    queryKey: ["validator_fees"],
    queryFn: () => validatorsGetFees({ account_or_key: accountOrKeyIndex, shard_group: shardGroup }),
    onError: (error: ApiError) => {
      error;
    },
  });
};
