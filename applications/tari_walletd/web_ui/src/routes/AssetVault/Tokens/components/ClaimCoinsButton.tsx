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
import { useAccountsCreateFreeTestCoins } from "@api/hooks/useAccounts";
import queryClient from "@api/queryClient";
import Button from "@mui/material/Button";
import { useTheme } from "@mui/material/styles";
import useMediaQuery from "@mui/material/useMediaQuery";
import useAccountStore, { setAccount, setOotleAddress } from "@store/accountStore";
import { AccountsCreateFreeTestCoinsResponse, substateIdToString } from "@tari-project/ootle-ts-bindings";

function ClaimCoinsButton() {
  const { mutate: claimTestnetFaucetFunds, isPending } = useAccountsCreateFreeTestCoins();
  const account = useAccountStore((s) => s.account);
  const { showError, showSuccess } = useErrorNotification();

  const theme = useTheme();
  const isLg = useMediaQuery(theme.breakpoints.up("md"));

  if (!account) {
    return <></>;
  }

  const onClaimFreeCoins = () => {
    claimTestnetFaucetFunds(
      {
        account: { ComponentAddress: substateIdToString(account.component_address) },
        amount: 1_000_000_000,
        fee: 1000,
      },
      {
        onSuccess: async (resp: AccountsCreateFreeTestCoinsResponse) => {
          setAccount(resp.account);
          setOotleAddress(resp.address);
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
          const errorMessage = error?.message || "Failed to claim testnet funds. Please try again.";
          showError(errorMessage);
        },
      },
    );
  };

  return (
    <Button variant="outlined" onClick={() => onClaimFreeCoins()} disabled={isPending} size={isLg ? "large" : "small"}>
      {isPending ? "Claiming..." : "Claim Testnet Funds"}
    </Button>
  );
}

export default ClaimCoinsButton;
