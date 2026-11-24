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
import { Avatar, Box, Button, Divider, Stack, Typography } from "@mui/material";
import { useNftTransferStore } from "@store/nftTransferStore";
import {
  convertCborValue,
  type Account,
  type NonFungibleId,
  type NonFungibleToken,
} from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";
import { displayNftId, formatCurrency, nftIdToString, substateIdToString } from "@utils/helpers";
import { findSelectedNft, parseMaxFeeAmount } from "./confirmationStepHelpers";

interface ConfirmationStepProps {
  accounts: Array<{ account: Account }> | undefined;
  preSelectedNftId?: NonFungibleId;
  availableNfts?: NonFungibleToken[];
  onBack: () => void;
  onConfirm: () => void;
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
  const selectedNft = findSelectedNft(availableNfts, preSelectedNftId);

  const nftMutableData = selectedNft ? convertCborValue(selectedNft.mutable_data) : null;
  const parsedMaxFee = parseMaxFeeAmount(transferFormState.maxFee);

  return (
    <Stack spacing={3} sx={{ py: 2 }}>
      <Stack direction="row" justifyContent="space-between" alignItems="center">
        <Stack spacing={4} direction={"row"}>
          <Stack spacing={2}>
            {preSelectedNftId && selectedNft ? (
              <Stack direction="row" spacing={2} alignItems="center">
                <Avatar
                  src={nftMutableData?.image_url}
                  sx={{
                    width: 215,
                    height: 215,
                    borderRadius: 1,
                    backgroundColor: "grey.200",
                  }}
                  variant="rounded"
                >
                  NFT
                </Avatar>
              </Stack>
            ) : (
              <Stack spacing={1}>
                <Typography variant="subtitle2" color="text.secondary">
                  You are about to send {transferFormState.nfts.length} NFT(s):
                </Typography>
                {transferFormState.nfts.map((nftId) => (
                  <Typography key={nftIdToString(nftId)} variant="body2">
                    • {displayNftId(nftId)}
                  </Typography>
                ))}
              </Stack>
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
              <Typography variant="subtitle1" component="div">
                <CopyAddress address={transferFormState.targetAccountAddress} />
              </Typography>
            </Box>

            <Box>
              <Typography variant="subtitle2" color="text.secondary">
                Transaction Fee:
              </Typography>
              <Typography>
                {parsedMaxFee !== null ? formatCurrency(parsedMaxFee, XTR_CURRENCY) : `-- ${XTR_CURRENCY.symbol}`}
              </Typography>
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
        <Button variant="outlined" onClick={onBack} disabled={disabled}>
          Back
        </Button>
        <Button variant="contained" onClick={onConfirm} disabled={disabled}>
          Confirm and Send
        </Button>
      </Stack>
    </Stack>
  );
}
