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
import { Box, Button, Divider, Stack, Typography } from "@mui/material";
import type { ResourceAddress, ResourceType } from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";
import { formatCurrency } from "@utils/helpers";
import { SendMoneyFormState } from "./FormStep";

interface ConfirmationStepProps {
  resource_address?: ResourceAddress;
  resource_type: ResourceType;
  transferFormState: SendMoneyFormState;
  disabled: boolean;
  onBack: () => void;
  onConfirm: () => void;
  token_symbol: string;
  divisibility: number;
}

export default function ConfirmationStep({
  resource_address,
  resource_type,
  transferFormState,
  disabled,
  onBack,
  onConfirm,
  token_symbol,
}: ConfirmationStepProps) {
  return (
    <Stack spacing={3} sx={{ py: 2 }}>
      <Stack spacing={2}>
        <Box>
          <Typography variant="h5" color="text.primary" gutterBottom>
            You are about to send:
          </Typography>
        </Box>

        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            Amount:
          </Typography>
          <Typography variant="body1">
            {transferFormState.amount}
            {token_symbol ? ` ${token_symbol}` : ""}
          </Typography>
        </Box>

        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            Transaction Fee:
          </Typography>
          <Typography variant="body1">{formatCurrency(parseInt(transferFormState.fee) || 0, XTR_CURRENCY)}</Typography>
        </Box>

        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            To Address:
          </Typography>
          <CopyAddress address={transferFormState.address} />
        </Box>

        <Box>
          <Typography variant="subtitle2" color="text.secondary">
            From:
          </Typography>
          <Typography variant="body1">{resource_address}</Typography>
        </Box>

        {resource_type === "Confidential" && (
          <Box>
            <Typography variant="subtitle2" color="text.secondary">
              Send Confidential Outputs:
            </Typography>
            <Typography variant="body1">{transferFormState.outputToRevealed ? "Yes" : "No"}</Typography>
          </Box>
        )}

        {transferFormState.badge && (
          <Box>
            <Typography variant="subtitle2" color="text.secondary">
              Using Badge:
            </Typography>
            <Typography variant="body1">{transferFormState.badge}</Typography>
          </Box>
        )}
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
