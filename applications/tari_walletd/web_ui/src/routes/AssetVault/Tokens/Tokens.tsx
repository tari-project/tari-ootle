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

import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import IconButton from "@mui/material/IconButton";
import { Typography, Box, Stack, Icon, Tooltip } from "@mui/material";
import { useState } from "react";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { DataTableCell } from "@components/StyledComponents";
import { useAccountsGetBalances } from "@api/hooks/useAccounts";
import useAccountStore from "@store/accountStore";
import { bigintToDecimalString, shortenSubstateId, substateIdToString } from "@utils/helpers";
import { Button } from "@mui/material";
import { SendMoneyDialog } from "./components/SendMoney";
import ClaimCoinsButton from "./components/ClaimCoinsButton";
import {
  ResourceAddress,
  ResourceType,
  VaultId,
  BalanceEntry,
  Account,
  Amount,
} from "@tari-project/typescript-bindings";
import CopyAddress from "@components/CopyAddress";
import { IoWalletOutline } from "react-icons/io5";
import { useNavigate } from "react-router-dom";

interface BalanceRowProps {
  token_symbol: string;
  resource_address: ResourceAddress;
  resource_type: ResourceType;
  vault_address?: VaultId;
  balance: Amount;
  confidential_balance: Amount;
  divisibility: number;
  onSendClicked?: (resource_address: ResourceAddress, resource_type: ResourceType) => void;
}

interface ConfidentialBalanceProps {
  show: boolean;
  balance: Amount;
  resourceType: string;
  divisibility: number;
  token_symbol?: string;
}

function ConfidentialBalance({ show, resourceType, balance, divisibility, token_symbol }: ConfidentialBalanceProps) {
  switch (resourceType) {
    case "Confidential":
    case "Stealth":
      return <>{show ? bigintToDecimalString(balance, divisibility) + " " + token_symbol : "**************"}</>;
    default:
      return <>--</>;
  }
}

function BalanceRow({
  token_symbol,
  resource_address,
  resource_type,
  balance,
  confidential_balance,
  vault_address,
  divisibility,
  onSendClicked,
}: BalanceRowProps) {
  const showBalance = useAccountStore((state) => state.showBalance);
  const navigate = useNavigate();
  return (
    <TableRow key={token_symbol || resource_address}>
      <DataTableCell>{vault_address ? <CopyAddress address={vault_address} /> : "--"}</DataTableCell>
      <DataTableCell>
        <CopyAddress
          address={resource_address}
          display={`${token_symbol || shortenSubstateId(resource_address)} ${resource_type}`}
        />
      </DataTableCell>
      <DataTableCell>
        {showBalance ? bigintToDecimalString(balance, divisibility) + " " + token_symbol : "*************"}
      </DataTableCell>
      <DataTableCell>
        <Stack direction="row" alignItems="center" gap={1}>
          <ConfidentialBalance
            show={showBalance}
            resourceType={resource_type}
            balance={confidential_balance}
            divisibility={divisibility}
            token_symbol={token_symbol}
          />
          {resource_type === "Stealth" && (
            <Tooltip title="View Stealth UTXOs">
              <IconButton size="small" onClick={() => navigate("/stealth-utxos")} color="primary">
                <IoWalletOutline />
              </IconButton>
            </Tooltip>
          )}
        </Stack>
      </DataTableCell>
      <DataTableCell>
        <Button variant="outlined" onClick={() => onSendClicked?.(resource_address, resource_type)}>
          Send
        </Button>
      </DataTableCell>
    </TableRow>
  );
}

function Tokens({ account }: { account: Account }) {
  const [resourceToSend, setResourceToSend] = useState<{
    address: ResourceAddress;
    resource_type: ResourceType;
  } | null>(null);

  const {
    data: balancesData,
    isError: balancesIsError,
    error: balancesError,
    isFetching: balancesIsFetching,
  } = useAccountsGetBalances(substateIdToString(account.component_address));

  const handleSendResourceClicked = (address: ResourceAddress, resource_type: ResourceType) => {
    setResourceToSend({ address, resource_type });
  };

  const hasBalances = balancesData?.balances && balancesData.balances.length > 0;

  return (
    <>
      {resourceToSend == null ? null : (
        <SendMoneyDialog
          open={true}
          handleClose={() => setResourceToSend(null)}
          onSendComplete={() => setResourceToSend(null)}
          resource_address={resourceToSend?.address}
          resource_type={resourceToSend?.resource_type!}
          token_symbol={
            balancesData?.balances.find((b: BalanceEntry) => b.resource_address === resourceToSend?.address)
              ?.token_symbol || ""
          }
        />
      )}
      <FetchStatusCheck
        isError={balancesIsError as boolean}
        errorMessage={(balancesError as { message?: string })?.message || "Error fetching data"}
        isLoading={(balancesIsFetching as boolean) && !balancesData}
      >
        <Stack gap={2} direction="column">
          <Stack direction="row" alignItems="center" justifyContent="flex-end">
            <ClaimCoinsButton />
          </Stack>

          {balancesData && !hasBalances ? (
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                py: 8,
                textAlign: "center",
              }}
            >
              <Typography variant="h6" color="text.secondary" gutterBottom>
                No tokens found
              </Typography>
              <Typography variant="body2" color="text.secondary">
                This account doesn't have any tokens yet. Try claiming some testnet coins to get started.
              </Typography>
            </Box>
          ) : (
            <TableContainer>
              <Table>
                <TableHead>
                  <TableRow>
                    <TableCell>Vault</TableCell>
                    <TableCell>Resource</TableCell>
                    <TableCell>Revealed Balance</TableCell>
                    <TableCell>Confidential Balance</TableCell>
                    <TableCell></TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {balancesData?.balances
                    .filter((b) => BigInt(b.balance) > 0n || BigInt(b.confidential_balance) > 0n)
                    .map(
                      (
                        {
                          resource_address,
                          balance,
                          resource_type,
                          confidential_balance,
                          token_symbol,
                          vault_address,
                          divisibility,
                        }: BalanceEntry,
                        i: number,
                      ) => (
                        <BalanceRow
                          key={i}
                          token_symbol={token_symbol || ""}
                          resource_address={resource_address}
                          resource_type={resource_type}
                          balance={balance}
                          confidential_balance={confidential_balance}
                          vault_address={vault_address ?? undefined} // convert null to undefined
                          divisibility={divisibility}
                          onSendClicked={handleSendResourceClicked}
                        />
                      ),
                    )}
                </TableBody>
              </Table>
            </TableContainer>
          )}
        </Stack>
      </FetchStatusCheck>
    </>
  );
}

export default Tokens;
