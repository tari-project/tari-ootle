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

import { Typography, Card, CardContent, Stack, Divider, Box, Button } from "@mui/material";
import type { NonFungibleId, Account } from "@tari-project/typescript-bindings";
import { substateIdToString, formatXTM } from "../../../../utils/helpers";
import { useNftTransferStore } from "../../../../store/nftTransferStore";
import CopyAddress from "../../../../Components/CopyAddress";

interface ConfirmationStepProps {
  accounts: Array<{ account: Account }> | undefined;
  preSelectedNftId?: NonFungibleId;
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

export default function ConfirmationStep({ accounts, preSelectedNftId, onBack, onConfirm }: ConfirmationStepProps) {
  const { transferFormState, disabled } = useNftTransferStore();

  return (
    <Stack spacing={3} sx={{ py: 2 }}>
      <Typography variant="h5">You are about to:</Typography>

      <Stack spacing={2}>
        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            Send:
          </Typography>
          <Typography>{preSelectedNftId ? nftIdToString(preSelectedNftId) : "Multiple NFTs"}</Typography>
        </Box>

        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            To Account:
          </Typography>
          <Typography variant="subtitle1">
            <CopyAddress address={transferFormState.targetAccountPublicKey} />
          </Typography>
        </Box>

        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            Transaction Fee:
          </Typography>
          <Typography>{formatXTM(parseInt(transferFormState.maxFee))}</Typography>
        </Box>

        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            Fee paid by:
          </Typography>
          <Typography>
            {accounts?.find((acc) => substateIdToString(acc.account.address) === transferFormState.payerAccount)
              ?.account.name || transferFormState.payerAccount}
          </Typography>
        </Box>
      </Stack>

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
