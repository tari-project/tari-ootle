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

import type { ApiError } from "@api/helpers/types";
import FetchStatusCheck from "@components/FetchStatusCheck";
import Button from "@mui/material/Button";
import Checkbox from "@mui/material/Checkbox";
import Grid from "@mui/material/Grid";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TablePagination from "@mui/material/TablePagination";
import TableRow from "@mui/material/TableRow";
import Typography from "@mui/material/Typography";
import type { ListNftsResponse, NonFungibleId, NonFungibleToken } from "@tari-project/ootle-ts-bindings";
import React, { useCallback, useState } from "react";
import { IoApps, IoList } from "react-icons/io5";
import ClaimNftsButton from "./components/ClaimNftsButton";
import { NftCard, NftRow } from "./components/NftParts";
import { TransferNftDialog } from "./components/SendNft";

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

function nftIdKey(nftId: NonFungibleId): string {
  return JSON.stringify(nftId);
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
  const [selectedMap, setSelectedMap] = useState<Map<string, NonFungibleToken>>(new Map());
  const [sendDialogOpen, setSendDialogOpen] = useState(false);
  const displayedNfts = nftsListData?.nfts || [];

  const toggleSelect = useCallback((nft: NonFungibleToken) => {
    setSelectedMap((prev) => {
      const key = nftIdKey(nft.nft_id);
      const next = new Map(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.set(key, nft);
      }
      return next;
    });
  }, []);

  const isAllPageSelected = displayedNfts.length > 0 && displayedNfts.every((nft) => selectedMap.has(nftIdKey(nft.nft_id)));

  const toggleSelectAll = useCallback(() => {
    setSelectedMap((prev) => {
      const next = new Map(prev);
      if (displayedNfts.every((nft) => next.has(nftIdKey(nft.nft_id)))) {
        displayedNfts.forEach((nft) => next.delete(nftIdKey(nft.nft_id)));
      } else {
        displayedNfts.forEach((nft) => next.set(nftIdKey(nft.nft_id), nft));
      }
      return next;
    });
  }, [displayedNfts]);

  const selectedNfts = Array.from(selectedMap.values());

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
          <NftCard
            key={`${nft.nft_id}-${index}`}
            nft={nft}
            selected={selectedMap.has(nftIdKey(nft.nft_id))}
            onToggleSelect={() => toggleSelect(nft)}
          />
        ))}
      </Grid>
    ) : (
      <TableContainer>
        <Table>
          <TableHead>
            <TableRow>
              <TableCell padding="checkbox">
                <Checkbox checked={isAllPageSelected} onChange={toggleSelectAll} />
              </TableCell>
              <TableCell>NFT</TableCell>
              <TableCell>Original Owner</TableCell>
              <TableCell>Status</TableCell>
              <TableCell>Actions</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {displayedNfts.map((nft: NonFungibleToken, index: number) => (
              <NftRow
                key={`${nft.nft_id}-${index}`}
                nft={nft}
                selected={selectedMap.has(nftIdKey(nft.nft_id))}
                onToggleSelect={() => toggleSelect(nft)}
              />
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
            {selectedNfts.length >= 2 && (
              <Button variant="contained" onClick={() => setSendDialogOpen(true)}>
                Send Selected ({selectedNfts.length})
              </Button>
            )}
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

      <TransferNftDialog
        open={sendDialogOpen}
        handleClose={() => setSendDialogOpen(false)}
        onSendComplete={() => {
          setSendDialogOpen(false);
          setSelectedMap(new Map());
        }}
        preSelectedNfts={selectedNfts}
      />
    </FetchStatusCheck>
  );
}
