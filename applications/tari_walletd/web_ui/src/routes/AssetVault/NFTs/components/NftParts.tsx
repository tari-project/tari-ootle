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

import CopyAddress from "@components/CopyAddress";
import { NftCard as Card, DataTableCell } from "@components/StyledComponents";
import CancelRoundedIcon from "@mui/icons-material/CancelRounded";
import CheckCircleRoundedIcon from "@mui/icons-material/CheckCircleRounded";
import {
  Avatar,
  Box,
  CardContent,
  CardMedia,
  Checkbox,
  Chip,
  Divider,
  Grid,
  Stack,
  TableCell,
  TableRow,
  Typography,
} from "@mui/material";
import { convertCborValue, type NonFungibleToken } from "@tari-project/ootle-ts-bindings";
import { displayNftId, shortenSubstateId } from "@utils/helpers";
import { Fragment } from "react/jsx-runtime";
import SendNft from "./SendNft";

const ERR_IMG =
  "data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMzAwIiBoZWlnaHQ9IjIwMCIgdmlld0JveD0iMCAwIDMwMCAyMDAiIGZpbGw9Im5vbmUiIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyI+CjxyZWN0IHdpZHRoPSIzMDAiIGhlaWdodD0iMjAwIiBmaWxsPSIjRjVGNUY1Ii8+CjxwYXRoIGQ9Ik0xMjUgNzVIMTc1VjEyNUgxMjVWNzVaIiBmaWxsPSIjRERERUREIi8+CjxwYXRoIGQ9Ik0xNDAgOTBIMTYwVjExMEgxNDBWOTBaIiBmaWxsPSIjQkJCQkJCIi8+Cjx0ZXh0IHg9IjE1MCIgeT0iMTQwIiBmb250LWZhbWlseT0iQXJpYWwiIGZvbnQtc2l6ZT0iMTIiIGZpbGw9IiM5OTk5OTkiIHRleHQtYW5jaG9yPSJtaWRkbGUiPk5GVDwvdGV4dD4KPC9zdmc+";

interface NftItemProps {
  nft: NonFungibleToken;
  selected?: boolean;
  selectDisabled?: boolean;
  onToggleSelect?: () => void;
}

function NftCard({ nft, selected, selectDisabled, onToggleSelect }: NftItemProps) {
  const mutableData = convertCborValue(nft.mutable_data);
  const data = convertCborValue(nft.data) as Record<string, any> | undefined;
  const imageUrl = mutableData?.image_url;

  return (
    <Grid size={{ xs: 12, sm: 6, md: 4, lg: 3 }}>
      <Card sx={{ position: "relative", opacity: selectDisabled ? 0.5 : 1 }}>
        {onToggleSelect && (
          <Checkbox
            checked={!!selected}
            disabled={selectDisabled}
            onChange={onToggleSelect}
            sx={{
              position: "absolute",
              top: 4,
              left: 4,
              zIndex: 1,
            }}
          />
        )}
        <CardMedia
          component="img"
          height="200"
          image={imageUrl || "/api/placeholder/300/200"}
          alt={`NFT ${displayNftId(nft.nft_id)}`}
          style={{ objectFit: "cover", backgroundColor: "grey.200" }}
          onError={(e: any) => {
            e.target.src = ERR_IMG;
          }}
        />
        <CardContent style={{ flexGrow: 1, display: "flex", flexDirection: "column", gap: 8 }}>
          <Box>
            <Typography variant="h6" component="h2" fontWeight="bold" noWrap>
              {displayNftId(nft.nft_id)}
            </Typography>
            {nft.is_burnt && (
              <Chip
                icon={<CancelRoundedIcon style={{ height: 16, width: 16 }} />}
                label={"Burnt"}
                color={"error"}
                size="small"
                variant="outlined"
              />
            )}
          </Box>
          <Divider />
          <Stack>
            <Typography variant="subtitle2">Vault:</Typography>
            <CopyAddress address={nft.vault_id} display={shortenSubstateId(nft.vault_id)} />
          </Stack>

          <Divider />
          {data ? <NftData data={data} /> : null}

          <SendNft nftId={nft.nft_id} resourceAddress={nft.resource_address} />
        </CardContent>
      </Card>
    </Grid>
  );
}

function NftData({ data }: { data: Record<string, any> }) {
  return (
    <>
      {Object.keys(data).map((key, i) => {
        const value = data[key];
        return (
          <Fragment key={i}>
            <Typography variant="subtitle2">{key}</Typography>
            <CopyAddress address={String(value)} />
          </Fragment>
        );
      })}
    </>
  );
}

function NftRow({ nft, selected, selectDisabled, onToggleSelect }: NftItemProps) {
  const mutableData = convertCborValue(nft.mutable_data);
  const data = convertCborValue(nft.data);
  const imageUrl = mutableData?.image_url;

  const metadata: [string, string][] = [];
  if (data && typeof data === "object") {
    let limit = 5;
    for (const [key, value] of Object.entries(data)) {
      if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
        metadata.push([key, value.toString()]);
        limit -= 1;
        if (limit === 0) break;
      }
    }
  }

  return (
    <TableRow>
      {onToggleSelect && (
        <TableCell padding="checkbox">
          <Checkbox checked={!!selected} disabled={selectDisabled} onChange={onToggleSelect} />
        </TableCell>
      )}
      <DataTableCell>
        <Box sx={{ display: "flex", alignItems: "center", gap: 2 }}>
          <Avatar
            src={imageUrl}
            style={{
              width: 60,
              height: 60,
              backgroundColor: "grey.200",
            }}
            variant="rounded"
          >
            NFT
          </Avatar>
          <Box>
            <Typography variant="subtitle2" fontWeight="bold">
              {displayNftId(nft.nft_id)}
            </Typography>
            <CopyAddress address={nft.vault_id} display={shortenSubstateId(nft.vault_id)} />
          </Box>
        </Box>
      </DataTableCell>
      <DataTableCell>
        {metadata.map(([key, value]) => (
          <Typography variant="body2" key={`meta_item_${key.slice(0, 4)}:${value}`}>
            <strong>{key}:</strong> {value}
          </Typography>
        ))}
      </DataTableCell>
      <DataTableCell>
        <Chip
          icon={
            nft.is_burnt ? (
              <CancelRoundedIcon style={{ height: 16, width: 16 }} />
            ) : (
              <CheckCircleRoundedIcon style={{ height: 16, width: 16 }} />
            )
          }
          label={nft.is_burnt ? "Burnt" : "Active"}
          color={nft.is_burnt ? "error" : "success"}
          size="small"
          variant="outlined"
        />
      </DataTableCell>
      <DataTableCell>
        <SendNft nftId={nft.nft_id} resourceAddress={nft.resource_address} />
      </DataTableCell>
    </TableRow>
  );
}

export { NftCard, NftRow };
