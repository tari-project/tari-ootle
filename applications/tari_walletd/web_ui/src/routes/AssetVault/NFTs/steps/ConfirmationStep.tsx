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

import { Box, Button, Stack, Typography, Avatar, Divider } from "@mui/material";
import type { Account, NonFungibleId, NonFungibleToken } from "@tari-project/typescript-bindings";
import CopyAddress from "@components/CopyAddress";
import { useNftTransferStore } from "@store/nftTransferStore";
import { formatCurrency, substateIdToString, displayNftId } from "@utils/helpers";
import { convertCborValue } from "@utils/cbor";

interface ConfirmationStepProps {
  accounts: Array<{ account: Account }> | undefined;
  preSelectedNftId?: NonFungibleId;
  availableNfts?: NonFungibleToken[];
  onBack: () => void;
  onConfirm: () => void;
}

function nftIdToString(nftId: NonFungibleId): string {
  const key = Object.keys(nftId)[0];
  // @ts-ignore
  const id = nftId[key].toString();
  const typeName = getNftIdTypeAsName(nftId);
  return typeName + "_" + id;
}

function getNftIdTypeAsName(nftId: NonFungibleId): string {
  const key = Object.keys(nftId)[0];
  switch (key) {
    case "U256":
      return "uuid";
    case "String":
      return "str";
    case "Uint32":
      return "u32";
    case "Uint64":
      return "u64";
    default:
      return "";
  }
}

export default function ConfirmationStep({
  accounts,
  preSelectedNftId,
  availableNfts,
  onBack,
  onConfirm,
}: ConfirmationStepProps) {
  const { transferFormState, disabled } = useNftTransferStore();

  // Find the NFT being transferred to show its image
  const selectedNft = availableNfts?.find(
    (nft) => preSelectedNftId && nftIdToString(nft.nft_id) === nftIdToString(preSelectedNftId),
  );

  const nftMutableData = selectedNft ? convertCborValue(selectedNft.mutable_data) : null;
  const nftImageUrl = nftMutableData?.image_url;

  return (
    <Stack spacing={3} sx={{ py: 2 }}>
      <Stack direction="row" justifyContent="space-between" alignItems="center">
        <Stack spacing={4} direction={"row"}>
          <Stack spacing={2}>
            {preSelectedNftId && selectedNft ? (
              <Stack direction="row" spacing={2} alignItems="center">
                <Avatar
                  src={nftImageUrl}
                  sx={{
                    width: 215,
                    height: 215,
                    borderRadius: 1,
                    backgroundColor: "grey.200",
                  }}
                  variant="rounded"
                  onError={(e: any) => {
                    e.target.src =
                      "data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iODAiIGhlaWdodD0iODAiIHZpZXdCb3g9IjAgMCA4MCA4MCIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHJlY3Qgd2lkdGg9IjgwIiBoZWlnaHQ9IjgwIiBmaWxsPSIjRjVGNUY1Ii8+CjxwYXRoIGQ9Ik0zMCAyNUg1MFY1NUgzMFYyNVoiIGZpbGw9IiNERERERUREIi8+CjxwYXRoIGQ9Ik0zNiAzMUg0NFY0M0gzNlYzMVoiIGZpbGw9IiNCQkJCQkIiLz4KPHR1eHQgeD0iNDAiIHk9IjUyIiBmb250LWZhbWlseT0iQXJpYWwiIGZvbnQtc2l6ZT0iOCIgZmlsbD0iIzk5OTk5OSIgdGV4dC1hbmNob3I9Im1pZGRsZSI+TkZUPC90ZXh0Pgo8L3N2Zz4K";
                  }}
                >
                  NFT
                </Avatar>
              </Stack>
            ) : (
              <Typography>{preSelectedNftId ? displayNftId(preSelectedNftId) : "Multiple NFTs"}</Typography>
            )}
          </Stack>
          <Stack spacing={2} direction={"column"}>
            {preSelectedNftId && (
              <Box>
                <Typography variant="subtitle2" color="text.secondary">
                  You are about to send:
                </Typography>
                <Typography variant="subtitle1">{displayNftId(preSelectedNftId)}</Typography>
              </Box>
            )}
            <Box>
              <Typography variant="subtitle2" color="text.secondary">
                To Account:
              </Typography>
              <Typography variant="subtitle1">
                <CopyAddress address={transferFormState.targetAccountAddress} />
              </Typography>
            </Box>

            <Box>
              <Typography variant="subtitle2" color="text.secondary">
                Transaction Fee:
              </Typography>
              <Typography>{formatCurrency(parseInt(transferFormState.maxFee))}</Typography>
            </Box>

            <Box>
              <Typography variant="subtitle2" color="text.secondary">
                Fee paid by:
              </Typography>
              <Typography>
                {accounts?.find(
                  (acc) => substateIdToString(acc.account.component_address) === transferFormState.payerAccount,
                )?.account.name || transferFormState.payerAccount}
              </Typography>
            </Box>
          </Stack>
        </Stack>
      </Stack>
      <Divider />
      <Stack direction="row" justifyContent="space-between" sx={{ mt: 3 }}>
        <Button variant="outlined" onClick={onBack}>
          Back
        </Button>
        <Button variant="contained" onClick={onConfirm} disabled={disabled}>
          Confirm and Send
        </Button>
      </Stack>
    </Stack>
  );
}
