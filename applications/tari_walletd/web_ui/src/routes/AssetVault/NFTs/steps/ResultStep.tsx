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

import { Typography, Stack, Button, CircularProgress, Fade, Divider } from "@mui/material";
import { useNftTransferStore } from "../../../../store/nftTransferStore";
import CancelRoundedIcon from "@mui/icons-material/CancelRounded";
import CheckCircleRoundedIcon from "@mui/icons-material/CheckCircleRounded";

interface ResultStepProps {
  onClose: () => void;
}

export default function ResultStep({ onClose }: ResultStepProps) {
  const { disabled, transferResult } = useNftTransferStore();

  return (
    <Stack direction="column" spacing={2} alignItems="center" justifyContent="center">
      {disabled ? (
        <>
          <CircularProgress size={60} />
          <Typography variant="h6">Sending NFT...</Typography>
          <Typography color="text.secondary">Please wait while your transaction is processed.</Typography>
        </>
      ) : transferResult ? (
        <>
          <Stack direction="column" alignItems="center" spacing={1}>
            {transferResult.success ? (
              <Fade in>
                <CheckCircleRoundedIcon sx={{ fontSize: 60, color: "success.main" }} />
              </Fade>
            ) : (
              <Fade in>
                <CancelRoundedIcon sx={{ fontSize: 60, color: "error.main" }} />
              </Fade>
            )}
            <Typography variant="h5">{transferResult.success ? "Transfer Successful!" : "Transfer Failed"}</Typography>
          </Stack>
          <Typography>{transferResult.message}</Typography>
          <Divider />
          <Button variant="contained" onClick={onClose}>
            Close
          </Button>
        </>
      ) : null}
    </Stack>
  );
}
