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

import { useMemo } from "react";
import { useParams } from "react-router-dom";
import {
  Grid,
  Stack,
  Typography,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Box,
} from "@mui/material";
import { NonFungibleSubstate, shortenString } from "@tari-project/ootle-ts-bindings";
import { convertCborValue } from "../../../utils/cbor";
import { useGetSubstate, useGetNonFungibles } from "../../../api/hooks/useSubstates";
import FetchStatusCheck from "../../../Components/FetchStatusCheck";
import CopyToClipboard from "../../../Components/CopyToClipboard";
import NftRow, { type NftData } from "./NftRow";
import { formatCurrency } from "../../../utils/helpers";

function Resources() {
  const { resourceAddress } = useParams();

  const {
    data: substateData,
    isLoading: substateLoading,
    isError: substateError,
    error: substateErrorMessage,
  } = useGetSubstate({
    address: resourceAddress,
    version: null,
    local_search_only: true,
    enabled: !!resourceAddress,
  });

  const resource = useMemo(() => {
    if (!substateData?.substate || !("Resource" in substateData.substate)) {
      return null;
    }
    return substateData.substate.Resource;
  }, [substateData]);

  const isNonFungible = resource?.resource_type === "NonFungible";

  const {
    data: nonFungiblesData,
    isLoading: nftLoading,
    isError: nftError,
    error: nftErrorMessage,
  } = useGetNonFungibles({
    address: resourceAddress,
    start_index: 0,
    end_index: 10,
    enabled: !!resourceAddress && isNonFungible,
  });

  const nfts = useMemo(() => {
    if (!nonFungiblesData?.non_fungibles) return [];

    const processedNfts: NftData[] = [];
    nonFungiblesData.non_fungibles.forEach((nft: NonFungibleSubstate) => {
      if (!("NonFungible" in nft.substate)) {
        console.error("NonFungible not found in substate");
        return;
      }
      let nftData;
      try {
        nftData = convertCborValue(nft.substate.NonFungible?.data);
      } catch (e) {
        console.error("Error converting CBOR value:", e);
        return;
      }
      if (nftData) {
        let mutableData;
        try {
          mutableData = convertCborValue(nft.substate.NonFungible?.mutable_data);
        } catch (e) {
          console.error("Error converting mutable CBOR value:", e);
        }

        const { name, amount } = nftData;
        const { image_url } = mutableData || {};
        const nftId = nft.address.id;
        const key = Object.keys(nftId)[0];
        const idValue = nftId[key as keyof typeof nftId];
        const address = `${key}_${idValue}`;
        const cleanTitle = name || `NFT ${idValue}`;

        processedNfts.push({
          img: image_url,
          title: cleanTitle,
          address,
          version: nft.version,
          nft,
          amount,
        });
      }
    });
    return processedNfts;
  }, [nonFungiblesData]);

  const isLoading = substateLoading || (isNonFungible && nftLoading);
  const isError = substateError || nftError;
  const errorMessage = substateErrorMessage || nftErrorMessage;

  const EmptyPlaceHolder = () => (
    <Stack alignItems="center" justifyContent="center" sx={{ py: 8 }}>
      <Typography variant="h6" color="text.secondary" gutterBottom>
        No NFTs found
      </Typography>
      <Typography variant="body2" color="text.secondary">
        This resource doesn't have any NFTs or they couldn't be loaded.
      </Typography>
    </Stack>
  );

  const DisplayNFTs = () => {
    return (
      <TableContainer>
        <Table>
          <TableHead>
            <TableRow>
              <TableCell>NFTs ({nfts.length})</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {nfts.map((nftData, index) => (
              <NftRow key={`${nftData.address}-${index}`} nftData={nftData} />
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    );
  };

  return (
    <FetchStatusCheck
      isLoading={isLoading}
      isError={isError}
      errorMessage={errorMessage ? (errorMessage as Error).message : "Error fetching resource data."}
    >
      <Stack direction="column">
        <Box sx={{ p: 2, backgroundColor: "background.paper", borderRadius: 1 }}>
          <Typography variant="h6" gutterBottom>
            Resource Information
          </Typography>
          <Grid container spacing={0}>
            <Grid item xs={12}>
              <Stack direction="row" alignItems="baseline" spacing={1}>
                <Typography variant="subtitle2">Address:</Typography>
                <Typography variant="body2" color="text.secondary">
                  {resourceAddress ? shortenString(resourceAddress) : ""}
                  <CopyToClipboard copy={resourceAddress || ""} />
                </Typography>
              </Stack>
            </Grid>
            <Grid item xs={12}>
              <Stack direction="row" alignItems="baseline" spacing={1}>
                <Typography variant="subtitle2">Token Symbol:</Typography>
                <Typography variant="body2" color="text.secondary">
                  {resource?.metadata?.SYMBOL || "<none>"}
                </Typography>
              </Stack>
            </Grid>
            <Grid item xs={12}>
              <Stack direction="row" alignItems="baseline" spacing={1}>
                <Typography variant="subtitle2">Resource Type:</Typography>
                <Typography variant="body2" color="text.secondary">
                  {resource?.resource_type}
                </Typography>
              </Stack>
            </Grid>
            <Grid item xs={12}>
              <Stack direction="row" alignItems="baseline" spacing={1}>
                <Typography variant="subtitle2">Total Supply:</Typography>
                <Typography variant="body2" color="text.secondary">
                  {resource?.total_supply ? formatCurrency(resource.total_supply, resource.divisibility, resource.metadata["SYMBOL"]): "--"}
                </Typography>
              </Stack>
            </Grid>
          </Grid>
        </Box>

        {isNonFungible && <>{nfts.length === 0 ? <EmptyPlaceHolder /> : <DisplayNFTs />}</>}
      </Stack>
    </FetchStatusCheck>
  );
}

export default Resources;
