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

import React, { useState } from "react";
import FetchStatusCheck from "../../../Components/FetchStatusCheck";
import {
  Card,
  CardContent,
  CardMedia,
  Grid,
  Typography,
  Box,
  Chip,
  TablePagination,
  Stack,
  IconButton,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Avatar,
  Divider,
} from "@mui/material";
import type { ApiError } from "../../../api/helpers/types";
import { shortenString, shortenSubstateId, toHexString } from "../../../utils/helpers";
import type { NonFungibleId, NonFungibleToken, ListNftsResponse, Account } from "@tari-project/typescript-bindings";
import { convertCborValue } from "../../../utils/cbor";
import { DataTableCell } from "../../../Components/StyledComponents";
import ClaimNftsButton from "./ClaimNftsButton";
import SendNft from "./SendNft";
import TransferNft from "../Components/TransferNft";
import CheckCircleRoundedIcon from "@mui/icons-material/CheckCircleRounded";
import CancelRoundedIcon from "@mui/icons-material/CancelRounded";
import CopyAddress from "../../../Components/CopyAddress";
import { IoApps, IoList } from "react-icons/io5";

function NftCard({ nft }: { nft: NonFungibleToken }) {
  const mutableData = convertCborValue(nft.mutable_data);
  const data = convertCborValue(nft.data);
  const imageUrl = mutableData?.image_url;
  const originalOwner = data?.original_owner;

  return (
    <Grid item xs={12} sm={6} md={4} lg={3}>
      <Card style={{ height: "100%", display: "flex", flexDirection: "column" }}>
        <CardMedia
          component="img"
          height="200"
          image={imageUrl || "/api/placeholder/300/200"}
          alt={`NFT ${displayNftId(nft.nft_id)}`}
          sx={{
            objectFit: "cover",
            backgroundColor: "grey.200",
          }}
          onError={(e: any) => {
            e.target.src =
              "data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMzAwIiBoZWlnaHQ9IjIwMCIgdmlld0JveD0iMCAwIDMwMCAyMDAiIGZpbGw9Im5vbmUiIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyI+CjxyZWN0IHdpZHRoPSIzMDAiIGhlaWdodD0iMjAwIiBmaWxsPSIjRjVGNUY1Ii8+CjxwYXRoIGQ9Ik0xMjUgNzVIMTc1VjEyNUgxMjVWNzVaIiBmaWxsPSIjRERERUREIi8+CjxwYXRoIGQ9Ik0xNDAgOTBIMTYwVjExMEgxNDBWOTBaIiBmaWxsPSIjQkJCQkJCIi8+Cjx0ZXh0IHg9IjE1MCIgeT0iMTQwIiBmb250LWZhbWlseT0iQXJpYWwiIGZvbnQtc2l6ZT0iMTIiIGZpbGw9IiM5OTk5OTkiIHRleHQtYW5jaG9yPSJtaWRkbGUiPk5GVDwvdGV4dD4KPC9zdmc+";
          }}
        />
        <CardContent sx={{ flexGrow: 1, display: "flex", flexDirection: "column", gap: 1 }}>
          <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 1 }}>
            <Typography variant="h6" component="h2" fontWeight="bold" noWrap>
              {displayNftId(nft.nft_id)}
            </Typography>
            <Chip
              icon={
                nft.is_burned ? (
                  <CancelRoundedIcon style={{ height: 16, width: 16 }} />
                ) : (
                  <CheckCircleRoundedIcon style={{ height: 16, width: 16 }} />
                )
              }
              label={nft.is_burned ? "Burned" : "Active"}
              color={nft.is_burned ? "error" : "success"}
              size="small"
              variant="outlined"
            />
          </Box>

          <Divider />
          <Typography variant="subtitle2">Vault:</Typography>
          <Typography variant="body2" color="text.secondary" gutterBottom>
            <CopyAddress address={nft.vault_id} display={shortenSubstateId(nft.vault_id)} />
          </Typography>

          <Divider />
          <Typography variant="subtitle2">Original Owner:</Typography>
          <Typography variant="body2" color="text.secondary" gutterBottom>
            <CopyAddress address={originalOwner || ""} />
          </Typography>

          <SendNft nftId={nft.nft_id} resourceAddress={nft.resource_address} />
        </CardContent>
      </Card>
    </Grid>
  );
}

function NftRow({ nft }: { nft: NonFungibleToken }) {
  const mutableData = convertCborValue(nft.mutable_data);
  const data = convertCborValue(nft.data);
  const imageUrl = mutableData?.image_url;
  const originalOwner = data?.original_owner;

  return (
    <TableRow>
      <DataTableCell>
        <Box sx={{ display: "flex", alignItems: "center", gap: 2 }}>
          <Avatar
            src={imageUrl}
            sx={{
              width: 60,
              height: 60,
              borderRadius: 1,
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
            <Typography variant="body2" color="text.secondary">
              <CopyAddress address={nft.vault_id} display={shortenSubstateId(nft.vault_id)} />
            </Typography>
          </Box>
        </Box>
      </DataTableCell>
      <DataTableCell>
        <Typography variant="body2">
          <CopyAddress address={originalOwner || ""} />
        </Typography>
      </DataTableCell>
      <DataTableCell>
        <Chip
          icon={
            nft.is_burned ? (
              <CancelRoundedIcon style={{ height: 16, width: 16 }} />
            ) : (
              <CheckCircleRoundedIcon style={{ height: 16, width: 16 }} />
            )
          }
          label={nft.is_burned ? "Burned" : "Active"}
          color={nft.is_burned ? "error" : "success"}
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

function displayNftId(nftId: NonFungibleId) {
  if ("U256" in nftId) {
    return `U256:${shortenString(toHexString(nftId.U256))}`;
  }
  if ("Uint64" in nftId) {
    return `Uint64:${nftId.Uint64}`;
  }
  if ("Uint32" in nftId) {
    return `Uint32:${nftId.Uint32}`;
  }
  if ("String" in nftId) {
    return `String:${nftId.String}`;
  }

  return JSON.stringify(nftId);
}

export interface NftListProps {
  nftsListIsError: boolean;
  nftsListIsFetching: boolean;
  nftsListError: ApiError | null;
  nftsListData?: ListNftsResponse;
  totalCount?: number;
  page: number;
  rowsPerPage: number;
  onPageChange: (event: unknown, newPage: number) => void;
  onRowsPerPageChange: (event: React.ChangeEvent<HTMLInputElement>) => void;
}

export default function NFTList(props: NftListProps) {
  const {
    nftsListIsError,
    nftsListIsFetching,
    nftsListError,
    nftsListData,
    totalCount,
    page,
    rowsPerPage,
    onPageChange,
    onRowsPerPageChange,
  } = props;

  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const displayedNfts = nftsListData?.nfts || [];
  const totalItems = totalCount || displayedNfts.length;

  return (
    <FetchStatusCheck
      isError={nftsListIsError}
      errorMessage={nftsListError?.message || "Error fetching data"}
      isLoading={nftsListIsFetching && !displayedNfts.length}
    >
      <Stack gap={2} direction="column">
        <Stack direction="row" alignItems="center" justifyContent="space-between">
          <Stack direction="row" spacing={1} alignItems="center">
            <IconButton
              onClick={() => setViewMode("grid")}
              color={viewMode === "grid" ? "primary" : "default"}
              size="small"
              sx={{
                backgroundColor: viewMode === "grid" ? "action.selected" : "transparent",
              }}
            >
              <IoApps />
            </IconButton>
            <IconButton
              onClick={() => setViewMode("list")}
              color={viewMode === "list" ? "primary" : "default"}
              size="small"
              sx={{
                backgroundColor: viewMode === "list" ? "action.selected" : "transparent",
              }}
            >
              <IoList />
            </IconButton>
          </Stack>

          <Stack direction="row" spacing={2} alignItems="center">
            <TransferNft />
            <ClaimNftsButton />
          </Stack>
        </Stack>

        {viewMode === "grid" ? (
          <Grid container spacing={3}>
            {displayedNfts.map((nft: NonFungibleToken, index: number) => (
              <NftCard key={`${nft.nft_id}-${index}`} nft={nft} />
            ))}
          </Grid>
        ) : (
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>NFT</TableCell>
                  <TableCell>Original Owner</TableCell>
                  <TableCell>Status</TableCell>
                  <TableCell>Actions</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {displayedNfts.map((nft: NonFungibleToken, index: number) => (
                  <NftRow key={`${nft.nft_id}-${index}`} nft={nft} />
                ))}
              </TableBody>
            </Table>
          </TableContainer>
        )}

        {totalItems > 0 && (
          <TablePagination
            component="div"
            count={totalItems}
            page={page}
            onPageChange={onPageChange}
            rowsPerPage={rowsPerPage}
            onRowsPerPageChange={onRowsPerPageChange}
            rowsPerPageOptions={[8, 12, 24, 48]}
          />
        )}
      </Stack>
    </FetchStatusCheck>
  );
}
