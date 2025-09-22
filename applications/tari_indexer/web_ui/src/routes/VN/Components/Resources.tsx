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

import { useEffect, useState } from "react";
import { getNonFungibles, getSubstate } from "../../../utils/json_rpc";
import { useParams } from "react-router-dom";
import {
  Grid,
  ImageList,
  ImageListItem,
  ImageListItemBar,
} from "@mui/material";
import {
  NonFungibleSubstate,
  ResourceAddress,
  substateIdToString,
} from "@tari-project/typescript-bindings";
import { convertCborValue } from "../../../utils/cbor";
import { Resource } from "@tari-project/typescript-bindings/dist/types/Resource";

interface NftData {
  img: string | null;
  title: string;
  address: string;
  version: number;
}

function Resources() {
  const [nfts, setNfts] = useState<NftData[]>([]);
  const [resource, setResource] = useState<Resource | null>(null);

  let { resourceAddress } = useParams();

  async function update(resourceAddress: ResourceAddress) {
    const substate = await getSubstate({
      address: resourceAddress,
      version: null,
      local_search_only: true,
    });
    if (!("Resource" in substate.substate)) {
      console.error("Resource not found in substate");
      return;
    }
    const resource = substate.substate.Resource;

    setResource(resource);

    if (resource.resource_type === "NonFungible") {
      const resp = await getNonFungibles({
        address: resourceAddress,
        start_index: 0,
        end_index: 10,
      });
      let nfts: NftData[] = [];
      resp.non_fungibles.forEach((nft: NonFungibleSubstate) => {
        console.log(nft);
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
          console.log(nftData);
          const { image_url, name } = nftData;
          const nftId = nft.address.id;
          const key = Object.keys(nftId)[0];
          const address = `${key}_${nftId[key as keyof typeof nftId]}`;
          nfts.push({
            img: image_url,
            title: name,
            address,
            version: nft.version,
          });
        }

        setNfts(nfts);
      });
    }
  }

  useEffect(() => {
    if (!resourceAddress) {
      return;
    }

    update(resourceAddress).catch((error) => {
      console.error("Error fetching resource data:", error);
    });
  }, [resourceAddress]);

  return (
    <Grid container spacing={2} direction="column" alignItems="left">
      <Grid item>
        <p>
          {resourceAddress} Token Symbol:{" "}
          {resource?.metadata?.SYMBOL || "<none>"}
        </p>
        <p>Resource Type: {resource?.resource_type}</p>
        <p>Total Supply: {resource?.total_supply}</p>
      </Grid>

      {nfts.length > 0 && (
        <ImageList cols={4} gap={8}>
          {nfts.map((item, i) =>
            item.img ? (
              <ImageListItem key={i}>
                <img
                  src={`${item.img}?size=248&fit=fill&auto=format`}
                  srcSet={`${item.img}?size=248&fit=fill&auto=format&dpr=2 4x`}
                  alt={item.title || "NFT image"}
                  loading="lazy"
                />
                <ImageListItemBar
                  title={item.title}
                  subtitle={
                    <span>
                      {item.address} v{item.version}
                    </span>
                  }
                  position="below"
                />
              </ImageListItem>
            ) : (
              <ImageListItem key={i}>
                <ImageListItemBar
                  title={item.title}
                  subtitle={
                    <span>
                      {item.address} v{item.version}
                    </span>
                  }
                  position="below"
                />
              </ImageListItem>
            ),
          )}
        </ImageList>
      )}
    </Grid>
  );
}

export default Resources;
