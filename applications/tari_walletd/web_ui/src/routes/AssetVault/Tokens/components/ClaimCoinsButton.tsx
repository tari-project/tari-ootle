//  Copyright 2025. The Tari Project
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

import { useErrorNotification } from "@/contexts/ErrorNotificationContext";
import { useAccountsCreateFreeTestCoins, useFaucetBalance } from "@api/hooks/useAccounts";
import { useWalletInfo } from "@api/hooks/useWalletInfo";
import queryClient from "@api/queryClient";
import CheckCircleOutlineIcon from "@mui/icons-material/CheckCircleOutline";
import Button from "@mui/material/Button";
import Tooltip from "@mui/material/Tooltip";
import { useTheme } from "@mui/material/styles";
import useMediaQuery from "@mui/material/useMediaQuery";
import useAccountStore, { setAccount, setOotleAddress } from "@store/accountStore";
import { AccountsCreateFreeTestCoinsResponse, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { settingsGet, settingsSet } from "@utils/json_rpc";
import { useCallback, useEffect, useRef, useState } from "react";
import { applyClaimSuccess, markAccountClaimed } from "./claimCoinsHelpers";

function ClaimCoinsButton() {
  const { mutate: claimTestnetFaucetFunds, isPending } = useAccountsCreateFreeTestCoins();
  const account = useAccountStore((s) => s.account);
  const { showError, showSuccess } = useErrorNotification();
  const [hasClaimed, setHasClaimed] = useState(false);
  const { data: walletInfo } = useWalletInfo();
  const isTestnet = !!walletInfo && walletInfo.network !== "mainnet";

  const theme = useTheme();
  const isLg = useMediaQuery(theme.breakpoints.up("md"));

  const accountAddress = account ? substateIdToString(account.component_address) : null;
  const activeAccountRef = useRef<string | null>(accountAddress);
  activeAccountRef.current = accountAddress;
  const { data: faucet } = useFaucetBalance(isTestnet && !!accountAddress && !hasClaimed);

  useEffect(() => {
    setHasClaimed(false);
    if (!accountAddress) return;
    let cancelled = false;
    settingsGet()
      .then((res) => {
        if (!cancelled) {
          setHasClaimed(res.claimed_accounts.includes(accountAddress));
        }
      })
      .catch(() => {
        if (!cancelled) setHasClaimed(false);
      });
    return () => {
      cancelled = true;
    };
  }, [accountAddress]);

  const markClaimed = useCallback(async (targetAddress: string) => {
    try {
      await markAccountClaimed(targetAddress, {
        getActiveAccount: () => activeAccountRef.current,
        setHasClaimed,
        getSettings: settingsGet,
        saveClaimedAccounts: (claimed_accounts) =>
          settingsSet({
            indexer_url: null,
            indexer_urls: null,
            advanced_ui_features: null,
            claimed_accounts,
          }),
      });
    } catch (e) {
      console.error("Failed to persist claimed account:", e);
    }
  }, []);

  if (!account || !accountAddress) {
    return <></>;
  }

  if (!isTestnet) {
    return <></>;
  }

  const faucetIsEmpty = !!faucet && BigInt(faucet.balance) < BigInt(faucet.claim_amount);

  const onClaimFreeCoins = () => {
    const claimedAddress = accountAddress;
    claimTestnetFaucetFunds(
      {
        account: { ComponentAddress: claimedAddress },
        // The claim creates the account and funds it, so it pays for a component, a vault, a badge
        // and the receipt. Allocated well above that — the excess is refunded, and too low a value
        // rejects the claim after the funds appear to have arrived.
        fee: 50_000,
      },
      {
        onSuccess: async (resp: AccountsCreateFreeTestCoinsResponse) => {
          applyClaimSuccess(claimedAddress, resp, {
            getActiveAccount: () => activeAccountRef.current,
            setAccount,
            setOotleAddress,
          });
          void markClaimed(claimedAddress);
          showSuccess("Successfully claimed testnet coins!");
          await queryClient.invalidateQueries({
            predicate: (query) => {
              const key = query.queryKey[0];
              return (
                typeof key === "string" &&
                (key === "balances" || key === "accounts_balances" || key.startsWith("accounts_get_balances"))
              );
            },
          });
        },
        onError: (error: any) => {
          console.error("Error claiming coins:", error);
          if ((error?.cause as any)?.code === 1002) {
            showError("The testnet faucet is empty. Burn on L1 to claim tTARI instead.");
          } else if ((error?.cause as any)?.code === 1001) {
            void markClaimed(claimedAddress);
            showError("You have already claimed your testnet funds. Each account can only claim once.");
          } else {
            showError(error?.message || "Failed to claim testnet funds. Please try again.");
          }
        },
      },
    );
  };

  if (hasClaimed) {
    return (
      <Button variant="outlined" disabled startIcon={<CheckCircleOutlineIcon />} size={isLg ? "large" : "small"}>
        Already Claimed
      </Button>
    );
  }

  if (faucetIsEmpty) {
    return (
      <Tooltip title="The testnet faucet is empty. Burn on L1 to claim tTARI instead.">
        <span>
          <Button variant="outlined" disabled size={isLg ? "large" : "small"}>
            Faucet Empty
          </Button>
        </span>
      </Tooltip>
    );
  }

  return (
    <Button variant="outlined" onClick={() => onClaimFreeCoins()} disabled={isPending} size={isLg ? "large" : "small"}>
      {isPending ? "Claiming..." : "Claim Testnet Funds"}
    </Button>
  );
}

export default ClaimCoinsButton;
