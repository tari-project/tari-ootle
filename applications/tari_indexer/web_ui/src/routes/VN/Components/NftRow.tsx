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

import { TableRow, Avatar, Box, Typography, Stack } from "@mui/material";
import { DataTableCell } from "../../../Components/StyledComponents";
import CopyToClipboard from "../../../Components/CopyToClipboard";
import { NonFungibleSubstate } from "@tari-project/typescript-bindings";
import { shortenString } from "./helpers";

export interface NftData {
  img: string | null;
  title: string;
  address: string;
  version: number;
  nft: NonFungibleSubstate;
  original_owner?: string;
  amount?: number;
}

function NftRow({ nftData }: { nftData: NftData }) {
  return (
    <TableRow>
      <DataTableCell>
        <Box sx={{ display: "flex", alignItems: "center", gap: 2 }}>
          <Avatar
            src={nftData.img || undefined}
            sx={{
              width: 60,
              height: 60,
              borderRadius: 1,
              backgroundColor: "grey.200",
            }}
            variant="rounded"
          >
            NFT Image
          </Avatar>
          <Stack spacing={0}>
            <Typography variant="subtitle2" fontWeight="bold" gutterBottom>
              {nftData.title}
            </Typography>
            <Stack direction="row" alignItems="baseline" spacing={1}>
              <Typography variant="body2">Original Owner:</Typography>
              <Typography variant="body2" color="text.secondary">
                {nftData.original_owner ? shortenString(nftData.original_owner) : "No owner"}
                {nftData.original_owner && <CopyToClipboard copy={nftData.original_owner} />}
              </Typography>
            </Stack>
            <Stack direction="row" alignItems="baseline" spacing={1}>
              <Typography variant="body2">Version:</Typography>
              <Typography variant="body2" color="text.secondary">
                v{nftData.version}
              </Typography>
            </Stack>
          </Stack>
        </Box>
      </DataTableCell>
    </TableRow>
  );
}

export default NftRow;
