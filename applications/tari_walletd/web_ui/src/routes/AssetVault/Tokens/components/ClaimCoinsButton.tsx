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

import { useCallback, useEffect, useState } from "react";
import { useErrorNotification } from "@/contexts/ErrorNotificationContext";
import { useAccountsCreateFreeTestCoins } from "@api/hooks/useAccounts";
import queryClient from "@api/queryClient";
import CheckCircleOutlineIcon from "@mui/icons-material/CheckCircleOutline";
import Button from "@mui/material/Button";
import { useTheme } from "@mui/material/styles";
import useMediaQuery from "@mui/material/useMediaQuery";
import useAccountStore, { setAccount, setOotleAddress } from "@store/accountStore";
import { AccountsCreateFreeTestCoinsResponse, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { settingsGet, settingsSet } from "@utils/json_rpc";

function ClaimCoinsButton() {
  const { mutate: claimTestnetFaucetFunds, isPending } = useAccountsCreateFreeTestCoins();
  const account = useAccountStore((s) => s.account);
  const { showError, showSuccess } = useErrorNotification();
  const [hasClaimed, setHasClaimed] = useState(false);

  const theme = useTheme();
  const isLg = useMediaQuery(theme.breakpoints.up("md"));

  const accountAddress = account ? substateIdToString(account.component_address) : null;

  useEffect(() => {
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

  const markClaimed = useCallback(async () => {
    if (!accountAddress) return;
    setHasClaimed(true);
    try {
      const current = await settingsGet();
      if (current.claimed_accounts.includes(accountAddress)) return;
      const updated = [...current.claimed_accounts, accountAddress];
      await settingsSet({
        indexer_url: null,
        advanced_ui_features: null,
        claimed_accounts: updated,
      });
    } catch (e) {
      console.error("Failed to persist claimed account:", e);
    }
  }, [accountAddress]);

  if (!account || !accountAddress) {
    return <></>;
  }

  const onClaimFreeCoins = () => {
    claimTestnetFaucetFunds(
      {
        account: { ComponentAddress: accountAddress },
        fee: 1000,
      },
      {
        onSuccess: async (resp: AccountsCreateFreeTestCoinsResponse) => {
          setAccount(resp.account);
          setOotleAddress(resp.address);
          markClaimed();
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
          if ((error?.cause as any)?.code === 1001) {
            markClaimed();
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

  return (
    <Button variant="outlined" onClick={() => onClaimFreeCoins()} disabled={isPending} size={isLg ? "large" : "small"}>
      {isPending ? "Claiming..." : "Claim Testnet Funds"}
    </Button>
  );
}

export default ClaimCoinsButton;
