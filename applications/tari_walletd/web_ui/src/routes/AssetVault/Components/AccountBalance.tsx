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

import { useAccountsGetBalances } from "@api/hooks/useAccounts";
import FetchStatusCheck from "@components/FetchStatusCheck";
import Box from "@mui/material/Box";
import Fade from "@mui/material/Fade";
import IconButton from "@mui/material/IconButton";
import Typography from "@mui/material/Typography";
import { useTheme } from "@mui/material/styles";
import useAccountStore from "@store/accountStore";
import { Account } from "@tari-project/ootle-ts-bindings";
import { substateIdToString } from "@utils/helpers";
import { IoEyeOffOutline, IoEyeOutline } from "react-icons/io5";
import { formatXtrBalance } from "./xtrBalanceHelpers";

export default function AccountBalance() {
  const showBalance = useAccountStore((state) => state.showBalance);
  const setShowBalance = useAccountStore((state) => state.setShowBalance);
  const account = useAccountStore((state) => state.account);

  if (!account) return <></>;

  return <AccountBalanceInner account={account} showBalance={showBalance} setShowBalance={setShowBalance} />;
}

function AccountBalanceInner({
  account,
  showBalance,
  setShowBalance,
}: {
  account: Account;
  showBalance: boolean;
  setShowBalance: (show: boolean) => void;
}) {
  const theme = useTheme();

  const {
    data: balancesData,
    isError: balancesIsError,
    error: balancesError,
    isLoading: balancesIsLoading,
  } = useAccountsGetBalances(substateIdToString(account.component_address));

  const { formattedBalance, symbol } = formatXtrBalance(balancesData?.balances, {
    mode: "total",
    showBalance,
    isLoading: balancesIsLoading,
  });

  return (
    <FetchStatusCheck
      isError={balancesIsError}
      errorMessage={balancesError?.message || "Error fetching data"}
      isLoading={false}
    >
      <Fade in={!balancesIsError} timeout={100}>
        <Box>
          <Box>
            <Typography variant="body2" gutterBottom={false}>
              Balance
            </Typography>
          </Box>
          <Box
            style={{
              display: "flex",
              alignItems: "flex-start",
              justifyContent: "space-between",
              gap: theme.spacing(1),
              minWidth: "250px",
            }}
          >
            <Typography variant="h2">
              {formattedBalance} <span style={{ fontSize: "18px" }}>{symbol}</span>
            </Typography>
            <IconButton
              aria-label={showBalance ? "Hide balance" : "Show balance"}
              onClick={() => setShowBalance(!showBalance)}
            >
              {showBalance ? (
                <IoEyeOffOutline color={theme.palette.primary.main} />
              ) : (
                <IoEyeOutline color={theme.palette.primary.main} />
              )}
            </IconButton>
          </Box>
        </Box>
      </Fade>
    </FetchStatusCheck>
  );
}
