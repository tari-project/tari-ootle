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

import {
  Grid,
  IconButton,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
  Typography,
  Box,
  Button,
} from "@mui/material";
import type { ListNftsResponse, NonFungibleToken } from "@tari-project/typescript-bindings";
import React, { useState } from "react";
import { IoApps, IoList } from "react-icons/io5";
import type { ApiError } from "@api/helpers/types";
import FetchStatusCheck from "@components/FetchStatusCheck";
import ClaimNftsButton from "./components/ClaimNftsButton";
import { NftCard, NftRow } from "./components/NftParts";

export interface NftListProps {
  nftsListIsError: boolean;
  nftsListIsFetching: boolean;
  nftsListError: ApiError | null;
  nftsListData?: ListNftsResponse;
  totalCount: number;
  page: number;
  rowsPerPage: number;
  onPageChange: (event: unknown, newPage: number) => void;
  onRowsPerPageChange: (event: React.ChangeEvent<HTMLInputElement>) => void;
  onManualRefresh?: () => void;
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
    onManualRefresh,
  } = props;

  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const displayedNfts = nftsListData?.nfts || [];

  const EmptyPlaceHolder = () => (
    <Stack alignItems="center" justifyContent="center" sx={{ py: 8 }}>
      <Typography variant="h6" color="text.secondary" gutterBottom>
        No NFTs found
      </Typography>
      <Typography variant="body2" color="text.secondary">
        You don't have any NFTs in this account yet. Try claiming some testnet NFTs to get started.
      </Typography>
    </Stack>
  );

  const FetchingPlaceHolder = () => (
    <Stack alignItems="center" justifyContent="center" sx={{ py: 8 }}>
      <Typography variant="h6" color="text.secondary" gutterBottom>
        NFTs loading...
      </Typography>
      <Typography variant="body2" color="text.secondary">
        Please wait while we fetch your NFTs from the wallet.
      </Typography>
      {onManualRefresh && (
        <Button variant="outlined" onClick={onManualRefresh} size="small" sx={{ mt: 2 }}>
          Click here to manually refresh
        </Button>
      )}
    </Stack>
  );

  const DisplayNFTs = () => {
    return viewMode === "grid" ? (
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
    );
  };

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
            <ClaimNftsButton />
          </Stack>
        </Stack>

        {totalCount === 0 ? (
          <EmptyPlaceHolder />
        ) : displayedNfts.length === 0 ? (
          <FetchingPlaceHolder />
        ) : (
          <DisplayNFTs />
        )}

        {totalCount > 0 && (
          <TablePagination
            component="div"
            count={totalCount}
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
