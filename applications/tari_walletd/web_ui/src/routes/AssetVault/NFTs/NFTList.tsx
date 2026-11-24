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
import type { ListNftsResponse, NonFungibleToken } from "@tari-project/ootle-ts-bindings";
import { nftIdToString } from "@utils/helpers";
import React, { useCallback, useEffect, useMemo, useState } from "react";
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

function nftKey(nft: NonFungibleToken): string {
  return `${nft.resource_address}:${nftIdToString(nft.nft_id)}`;
}

function EmptyPlaceholder() {
  return (
    <Stack alignItems="center" justifyContent="center" sx={{ py: 8 }}>
      <Typography variant="h6" color="text.secondary" gutterBottom>
        No NFTs found
      </Typography>
      <Typography variant="body2" color="text.secondary">
        You don't have any NFTs in this account yet. Try claiming some testnet NFTs to get started.
      </Typography>
    </Stack>
  );
}

function FetchingPlaceholder({ onManualRefresh }: { onManualRefresh?: () => void }) {
  return (
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
}

interface NftCollectionProps {
  viewMode: "grid" | "list";
  displayedNfts: NonFungibleToken[];
  selectedMap: Map<string, NonFungibleToken>;
  isSelectDisabled: (nft: NonFungibleToken) => boolean;
  onToggleSelect: (nft: NonFungibleToken) => void;
}

function NftCollection({ viewMode, displayedNfts, selectedMap, isSelectDisabled, onToggleSelect }: NftCollectionProps) {
  return viewMode === "grid" ? (
    <Grid container spacing={3}>
      {displayedNfts.map((nft: NonFungibleToken) => {
        const key = nftKey(nft);
        return (
          <NftCard
            key={key}
            nft={nft}
            selected={selectedMap.has(key)}
            selectDisabled={isSelectDisabled(nft)}
            onToggleSelect={() => onToggleSelect(nft)}
          />
        );
      })}
    </Grid>
  ) : (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell padding="checkbox" aria-hidden />
            <TableCell>NFT</TableCell>
            <TableCell>Original Owner</TableCell>
            <TableCell>Status</TableCell>
            <TableCell>Actions</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {displayedNfts.map((nft: NonFungibleToken) => {
            const key = nftKey(nft);
            return (
              <NftRow
                key={key}
                nft={nft}
                selected={selectedMap.has(key)}
                selectDisabled={isSelectDisabled(nft)}
                onToggleSelect={() => onToggleSelect(nft)}
              />
            );
          })}
        </TableBody>
      </Table>
    </TableContainer>
  );
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
  const lockedResourceAddress = selectedMap.size ? Array.from(selectedMap.values())[0].resource_address : null;
  const maxPage = totalCount > 0 ? Math.ceil(totalCount / rowsPerPage) - 1 : 0;
  const clampedPage = Math.min(page, maxPage);

  const toggleSelect = useCallback((nft: NonFungibleToken) => {
    setSelectedMap((prev) => {
      const key = nftKey(nft);
      const next = new Map(prev);
      next.has(key) ? next.delete(key) : next.set(key, nft);
      return next;
    });
  }, []);

  const isSelectDisabled = useCallback(
    (nft: NonFungibleToken) => {
      return (
        lockedResourceAddress !== null &&
        !selectedMap.has(nftKey(nft)) &&
        nft.resource_address !== lockedResourceAddress
      );
    },
    [lockedResourceAddress, selectedMap],
  );

  // Prune selections that are no longer in the displayed NFT list
  useEffect(() => {
    setSelectedMap((prev) => {
      if (prev.size === 0) return prev;
      const currentKeys = new Set(displayedNfts.map(nftKey));
      let changed = false;
      const next = new Map<string, NonFungibleToken>();
      for (const [key, nft] of prev) {
        if (currentKeys.has(key)) {
          next.set(key, nft);
        } else {
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [displayedNfts]);

  const selectedNfts = useMemo(() => Array.from(selectedMap.values()), [selectedMap]);

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
            {selectedNfts.length > 0 && (
              <Button variant="contained" onClick={() => setSendDialogOpen(true)}>
                Send Selected ({selectedNfts.length})
              </Button>
            )}
            <ClaimNftsButton />
          </Stack>
        </Stack>

        {totalCount === 0 ? (
          <EmptyPlaceholder />
        ) : displayedNfts.length === 0 ? (
          <FetchingPlaceholder onManualRefresh={onManualRefresh} />
        ) : (
          <NftCollection
            viewMode={viewMode}
            displayedNfts={displayedNfts}
            selectedMap={selectedMap}
            isSelectDisabled={isSelectDisabled}
            onToggleSelect={toggleSelect}
          />
        )}

        {totalCount > 0 && (
          <TablePagination
            component="div"
            count={totalCount}
            page={clampedPage}
            onPageChange={onPageChange}
            rowsPerPage={rowsPerPage}
            onRowsPerPageChange={onRowsPerPageChange}
            rowsPerPageOptions={[8, 12, 24, 48]}
          />
        )}
      </Stack>

      {sendDialogOpen && (
        <TransferNftDialog
          open
          handleClose={() => setSendDialogOpen(false)}
          onSendComplete={() => {
            setSendDialogOpen(false);
            setSelectedMap(new Map());
          }}
          preSelectedNfts={selectedNfts}
          preSelectedResourceAddress={lockedResourceAddress ?? undefined}
        />
      )}
    </FetchStatusCheck>
  );
}
