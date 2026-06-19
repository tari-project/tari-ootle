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

import { ApiError } from "@api/helpers/types";
import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import {
  AccountOrKeyId,
  BadgeUsage,
  BalanceChangeSourceType,
  ClaimBurnRequest,
  ComponentAddress,
  ComponentAddressOrName,
  decodeOotleAddress,
  Memo,
  OutputStatus,
  PayTo,
  ResourceAddress,
  ResourceType,
  TARI_TOKEN,
  UtxoInputSelection,
} from "@tari-project/ootle-ts-bindings";
import {
  accountsClaimBurn,
  accountsConfidentialTransfer,
  accountsCreate,
  accountsCreateFreeTestCoins,
  accountsGet,
  accountsGetBalanceChanges,
  accountsGetBalances,
  accountsGetDefault,
  accountsList,
  accountsRename,
  accountsStealthTransfer,
  accountsTransfer,
  mintFaucetNfts,
  stealthUtxosList,
  validatorsGetFees,
} from "@utils/json_rpc";

//   Fees are passed as strings because Amount is tagged
export const useAccountsClaimBurn = () => {
  return useMutation({
    mutationFn: (params: ClaimBurnRequest) => accountsClaimBurn(params),
    onError: (error: ApiError) => {
      error;
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["accounts"] });
    },
  });
};

export type AccountsCreateMutate = {
  accountName?: string;
  isDefault?: boolean;
  keyId?: number | null;
};

export const useAccountsCreate = () => {
  return useMutation({
    mutationFn: async (req: AccountsCreateMutate) => {
      return await accountsCreate({
        account_name: req.accountName || "",
        is_default: req.isDefault || null,
        key_index: req.keyId ?? null,
      });
    },
    onError: (error: ApiError) => {
      error;
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["accounts"] });
    },
  });
};

export type AccountsRenameMutate = {
  account: ComponentAddress;
  newName: string;
};

export const useAccountsRename = () => {
  return useMutation({
    mutationFn: async (req: AccountsRenameMutate) => {
      return await accountsRename({
        account: { ComponentAddress: req.account },
        new_name: req.newName,
      });
    },
    onError: (error: ApiError) => {
      error;
    },
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: ["accounts"] });
      queryClient.invalidateQueries({ queryKey: [`accounts_get_${variables.account}`] });
      queryClient.refetchQueries({ queryKey: ["accounts"] });
    },
  });
};

export interface TransferParams {
  account: ComponentAddress;
  amount: bigint | string;
  resource_address: string;
  destination_address: string;
  max_fee: number | null;
  resourceType: ResourceType;
  output_to_revealed: boolean;
  input_selection: UtxoInputSelection;
  badge_usage: BadgeUsage;
  dry_run: boolean;
  output_memo?: Memo;
  attach_sender_address?: boolean;
  pay_ref?: string | null;
  swap_pool_address?: string | null;
  swap_input_amount?: bigint | null;
}

export const useAccountsTransfer = () => {
  return useMutation({
    mutationFn: (params: TransferParams) => {
      const account = { ComponentAddress: params.account };
      const max_fee = BigInt(params.max_fee || 1);
      const parsedAddress = decodeOotleAddress(params.destination_address);
      if (params.resourceType === "Confidential") {
        let transferRequest = {
          account,
          amount: params.amount,
          resource_address: params.resource_address,
          destination_address: params.destination_address,
          max_fee,
          // TODO: we only support Resource badge usage for confidential transfers for now
          proof_from_badge_resource:
            typeof params.badge_usage === "object" && "Resource" in params.badge_usage
              ? params.badge_usage.Resource
              : null,
          input_selection: params.input_selection,
          output_to_revealed: params.output_to_revealed,
          output_memo: params.output_memo || null,
          dry_run: params.dry_run,
        };
        return accountsConfidentialTransfer(transferRequest);
      } else if (params.resourceType === "Stealth") {
        let transferRequest = {
          owner_account: account,
          fee_params: {
            // For simplicity, we'll use prefer revealed for fees whenever a non-TARI_TOKEN stealth transfer is made
            input_selection: params.resource_address === TARI_TOKEN ? params.input_selection : "PreferRevealed",
            pay_fee_with_swap: params.swap_pool_address
              ? {
                  pool_address: params.swap_pool_address,
                  input_resource: params.resource_address,
                  input_amount: params.swap_input_amount || 0n,
                  // Set min output to max_fee so the swap must produce enough TARI to cover the fee.
                  // Any excess TARI from the swap is deposited back to the user's account.
                  min_xtr_output_amount: max_fee,
                }
              : null,
          },
          input_selection: params.input_selection,
          resource_address: params.resource_address,
          badge_usage: params.badge_usage,
          transfers: [
            {
              destination_address: params.destination_address,
              blinded_output_amount: params.output_to_revealed ? 0n : params.amount,
              revealed_output_amount: params.output_to_revealed ? params.amount : 0,
              output_memo: params.output_memo || null,
              pay_to: "StealthPublicKey" as PayTo,
              attach_sender_address: params.attach_sender_address ?? false,
              pay_ref: params.pay_ref ?? null,
            },
          ],
          max_fee,
          dry_run: params.dry_run,
        };
        return accountsStealthTransfer(transferRequest);
      } else {
        // Fungible and NFTs
        let transferRequest = {
          account,
          amount: params.amount,
          resource_address: params.resource_address,
          destination_public_key: parsedAddress.accountPublicKey,
          max_fee,
          // TODO: we only support Resource badge usage for public fungible transfers for now
          proof_from_badge_resource:
            typeof params.badge_usage === "object" && "Resource" in params.badge_usage
              ? params.badge_usage.Resource
              : null,
          input_selection: params.input_selection,
          output_to_revealed: params.output_to_revealed,
          dry_run: params.dry_run,
        };
        return accountsTransfer(transferRequest);
      }
    },
    onError: (error: ApiError) => {
      error;
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["accounts"] });
    },
  });
};

export const useAccountsCreateFreeTestCoins = () => {
  const createFreeTestCoins = async ({ account, fee }: { account: ComponentAddressOrName; fee: number }) =>
    accountsCreateFreeTestCoins({
      account,
      max_fee: BigInt(fee),
    });

  return useMutation({
    mutationFn: createFreeTestCoins,
    onError: (error: ApiError) => {
      console.error(error);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["transactions"] });
      queryClient.invalidateQueries({ queryKey: ["accounts_balances"] });
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
    maxFee: number;
  }) =>
    mintFaucetNfts({
      account,
      mutable_data: mutableData,
      number_to_mint: BigInt(numberToMint),
      max_fee: BigInt(maxFee),
    });

  return useMutation({
    mutationFn: callApi,
    onError: (error: ApiError) => {
      console.error(error);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["transactions"] });
      queryClient.invalidateQueries({ queryKey: ["accounts_balances"] });

      // Delayed invalidation for NFTs to handle wallet processing time
      setTimeout(() => {
        queryClient.invalidateQueries({
          predicate: (query) => {
            const key = query.queryKey[0];
            return typeof key === "string" && (key === "nfts" || key === "list_nfts" || key === "nfts_list");
          },
        });
      }, 1500);
    },
  });
};

export const useAccountsList = (offset: number, limit: number, enabled: boolean = true) => {
  return useQuery({
    queryKey: ["accounts"],
    queryFn: () => accountsList({ offset, limit }),
    enabled,
  });
};

export const useAccountsGetBalances = (account?: ComponentAddress, refresh: boolean = false) => {
  return useQuery({
    enabled: !!account,
    queryKey: [`accounts_balances_${account}`],
    queryFn: () => accountsGetBalances({ account: { ComponentAddress: account! }, refresh }),
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

export const useAccountsGetBalanceChanges = (
  account: ComponentAddress | undefined,
  offset: number,
  limit: number,
  resourceAddress?: ResourceAddress,
  sourceType?: BalanceChangeSourceType,
) => {
  return useQuery({
    enabled: !!account,
    queryKey: ["account_balance_changes", account, resourceAddress, sourceType, offset, limit],
    queryFn: () =>
      accountsGetBalanceChanges({
        account: { ComponentAddress: account! },
        offset,
        limit,
        resource_address: resourceAddress ?? null,
        transaction_id: null,
        source_type: sourceType ?? null,
      }),
    refetchInterval: 5000,
  });
};

export const refreshAccountsBalances = () => {
  return useMutation({
    mutationFn: (account: ComponentAddress) =>
      accountsGetBalances({ account: { ComponentAddress: account }, refresh: true }),
    onError: (error: ApiError) => {
      error;
    },
    onSettled: (resp) => {
      if (resp) {
        return queryClient.invalidateQueries({ queryKey: ["accounts_balances_" + resp.address] });
      }
    },
  });
};

export const useAccountsGetDefault = (enabled: boolean = true) => {
  return useQuery({
    enabled,
    queryKey: ["accounts_get_default"],
    queryFn: () => accountsGetDefault({}),
    staleTime: 0,
    refetchInterval: false,
    refetchOnMount: "always",
    notifyOnChangeProps: ["data", "error"],
    retryOnMount: false,
    retry: false,
  });
};
export const useAccountsGet = (account: ComponentAddress) => {
  return useQuery({
    queryKey: ["accounts_get_" + account],
    queryFn: () => accountsGet({ name_or_address: { ComponentAddress: account } }),
  });
};

// export const useAccountNFTsList = (account: ComponentAddress, offset: number, limit: number) => {
//   return useQuery({
//     queryKey: ["nfts_list", account, offset, limit],
//     queryFn: () => nftList({ account: { ComponentAddress: account }, offset, limit }),
//   });
// };

export const useValidatorFees = (accountOrKeyId: AccountOrKeyId, shardGroup = null) => {
  return useQuery({
    queryKey: ["validator_fees"],
    queryFn: () => validatorsGetFees({ account_or_key: accountOrKeyId, shard_group: shardGroup }),
  });
};

// (alias) type StealthUtxosListRequest = {
//     resource_address: ResourceAddress;
//     account_address: ComponentAddress | null;
//     filter_by_status: OutputStatus | null;
// }

export const useStealthUtxosList = (
  account_address: ComponentAddress,
  resource_address: ResourceAddress,
  filter_by_status: OutputStatus | null,
) => {
  return useQuery({
    queryKey: ["stealth_utxos_list", account_address, resource_address, filter_by_status],
    queryFn: () =>
      stealthUtxosList({
        account_address,
        resource_address,
        filter_by_status,
      }),
    enabled: !!account_address && !!resource_address,
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
