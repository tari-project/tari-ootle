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

import CopyAddress from "@components/CopyAddress";
import { Box, Stack, Tooltip, Typography } from "@mui/material";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";

import { useAccountsGetBalances } from "@api/hooks/useAccounts";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { FluidTableCell } from "@components/StyledComponents";
import Button from "@mui/material/Button";
import { useTheme } from "@mui/material/styles";
import useMediaQuery from "@mui/material/useMediaQuery";
import TypeChip from "@routes/AssetVault/Components/ResourceTypeChip";
import { TransferNftDialog } from "@routes/AssetVault/NFTs/components/SendNft";
import useAccountStore from "@store/accountStore";
import { Account, Amount, BalanceEntry, ResourceAddress, ResourceType, VaultId } from "@tari-project/ootle-ts-bindings";
import { bigintToDecimalString, shortenString, substateIdToString } from "@utils/helpers";
import { useState } from "react";
import { IoDocumentTextOutline, IoWalletOutline } from "react-icons/io5";
import { useNavigate } from "react-router-dom";
import ClaimCoinsButton from "./components/ClaimCoinsButton";
import { SendMoneyDialog } from "./components/SendMoney";

interface BalanceRowProps {
  token_symbol: string;
  resource_address: ResourceAddress;
  resource_type: ResourceType;
  vault_address?: VaultId;
  balance: Amount;
  confidential_balance: Amount;
  divisibility: number;
  accountAddress?: string;
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
      return <>{show ? bigintToDecimalString(balance, divisibility) + " " + token_symbol : "********"}</>;
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
  accountAddress,
  onSendClicked,
}: BalanceRowProps) {
  const theme = useTheme();
  const navigate = useNavigate();
  const isLg = useMediaQuery(theme.breakpoints.up("md"));
  const showBalance = useAccountStore((state) => state.showBalance);

  const stealthUXTOs = resource_type === "Stealth" && (
    <Tooltip title="View Stealth UTXOs">
      <Button
        variant="outlined"
        size="small"
        endIcon={<IoWalletOutline />}
        onClick={() => navigate(`/stealth-utxos/${resource_address}`)}
      >
        View
      </Button>
    </Tooltip>
  );

  const balanceChangesButton = accountAddress && (
    <Tooltip title="View balance changes for this resource">
      <Button
        variant="outlined"
        size="small"
        endIcon={<IoDocumentTextOutline />}
        onClick={() => navigate(`/accounts/${accountAddress}`)}
      >
        Changes
      </Button>
    </Tooltip>
  );

  return (
    <TableRow key={token_symbol || resource_address}>
      <FluidTableCell>{vault_address ? <CopyAddress address={vault_address} /> : "--"}</FluidTableCell>
      <FluidTableCell>
        <Stack
          direction={{
            xs: "column-reverse",
            md: "row",
          }}
          gap={isLg ? 1 : 0.3}
        >
          <CopyAddress
            address={resource_address}
            display={shortenString(resource_address, isLg ? 3 : 1, isLg ? 6 : 4)}
          />
          <TypeChip type={resource_type} symbol={token_symbol} compact />
        </Stack>
      </FluidTableCell>
      <FluidTableCell align="right">
        {showBalance ? `${bigintToDecimalString(balance, divisibility)} ${token_symbol}` : "********"}
      </FluidTableCell>
      <FluidTableCell align="right">
        <ConfidentialBalance
          show={showBalance}
          resourceType={resource_type}
          balance={confidential_balance}
          divisibility={divisibility}
          token_symbol={token_symbol}
        />
      </FluidTableCell>
      <FluidTableCell align="right">
        <Stack direction="row" gap={1} justifyContent="flex-end">
          <Button size="small" variant="outlined" onClick={() => onSendClicked?.(resource_address, resource_type)}>
            Send
          </Button>
          {balanceChangesButton}
          {stealthUXTOs}
        </Stack>
      </FluidTableCell>
    </TableRow>
  );
}

function Tokens({ account }: { account: Account }) {
  const theme = useTheme();
  const isLg = useMediaQuery(theme.breakpoints.up("md"));
  const [resourceToSend, setResourceToSend] = useState<{
    address: ResourceAddress;
    resource_type: ResourceType;
  } | null>(null);
  const [nftResourceToSend, setNftResourceToSend] = useState<ResourceAddress | null>(null);

  const {
    data: balancesData,
    isError: balancesIsError,
    error: balancesError,
    isFetching: balancesIsFetching,
  } = useAccountsGetBalances(substateIdToString(account.component_address));

  const handleSendResourceClicked = (address: ResourceAddress, resource_type: ResourceType) => {
    if (resource_type === "NonFungible") {
      setNftResourceToSend(address);
    } else {
      setResourceToSend({ address, resource_type });
    }
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
      {nftResourceToSend != null && (
        <TransferNftDialog
          open={true}
          handleClose={() => setNftResourceToSend(null)}
          onSendComplete={() => setNftResourceToSend(null)}
          preSelectedResourceAddress={nftResourceToSend}
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
                    <TableCell size="small">Vault</TableCell>
                    <TableCell size="small">Resource</TableCell>
                    <TableCell size="small" align="right">
                      Revealed Balance
                    </TableCell>
                    <TableCell size="small" align="right">
                      Confidential Balance
                    </TableCell>
                    <TableCell size="small" align="right"></TableCell>
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
                          accountAddress={substateIdToString(account.component_address)}
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
