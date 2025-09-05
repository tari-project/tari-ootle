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

import Box from "@mui/material/Box";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import Typography from "@mui/material/Typography";
import React, { useState } from "react";
import { useAccountNFTsList } from "@api/hooks/useAccounts";
import { ApiError } from "@api/helpers/types";
import { useListNfts } from "@api/hooks/useNfts";
import { substateIdToString, handleChangePage, handleChangeRowsPerPage } from "@utils/helpers";
import NFTList from "@routes/AssetVault/NFTs/NFTList";
import { Account } from "@tari-project/typescript-bindings";
import Tokens from "@routes/AssetVault/Tokens/Tokens";

interface TabPanelProps {
  children?: React.ReactNode;
  index: number;
  value: number;
}

function TabPanel(props: TabPanelProps) {
  const { children, value, index, ...other } = props;

  return (
    <div
      role="tabpanel"
      hidden={value !== index}
      id={`simple-tabpanel-${index}`}
      aria-labelledby={`simple-tab-${index}`}
      {...other}
    >
      {value === index && (
        <Box sx={{ p: 3 }}>
          <Typography component="div">{children}</Typography>
        </Box>
      )}
    </div>
  );
}

function tabProps(index: number) {
  return {
    "id": `asset-tab-${index}`,
    "aria-controls": `asset-tabpanel-${index}`,
  };
}

function Assets({ account }: { account: Account }) {
  const [assetTab, setAssetTab] = useState(0);
  const [nftPage, setNftPage] = useState(0);
  const [nftRowsPerPage, setNftRowsPerPage] = useState(12);

  // Reset pagination and tab when account changes
  React.useEffect(() => {
    setNftPage(0);
    setAssetTab(0);
  }, [account]);

  const {
    data: nftsListData,
    isError: nftsListIsError,
    error: nftsListError,
    isFetching: nftsListIsFetching,
  } = useAccountNFTsList(substateIdToString(account.address), nftPage * nftRowsPerPage, nftRowsPerPage);

  // Get total count of NFTs for accurate pagination
  const { data: allNfts } = useListNfts({
    account: { ComponentAddress: substateIdToString(account.address) },
  });

  // Calculate total count - use actual count from allNfts, fallback to estimation
  const currentNfts = nftsListData?.nfts || [];
  const actualTotal = allNfts ? allNfts.length : null;

  // Use actual total if available, otherwise fall back to simple estimation
  const totalCount = actualTotal !== null ? actualTotal : currentNfts.length;

  const handleChange = (_event: React.SyntheticEvent, newValue: number) => {
    setAssetTab(newValue);
  };

  return (
    <Box sx={{ width: "100%" }}>
      <Box sx={{ borderBottom: 1, borderColor: "divider" }}>
        <Tabs value={assetTab} onChange={handleChange} aria-label="account assets" variant="standard">
          <Tab label="Tokens" {...tabProps(0)} style={{ width: 150 }} />
          <Tab label="NFTs" {...tabProps(1)} style={{ width: 150 }} />
        </Tabs>
      </Box>
      <TabPanel value={assetTab} index={0}>
        <Tokens account={account} />
      </TabPanel>
      <TabPanel value={assetTab} index={1}>
        <NFTList
          nftsListIsError={nftsListIsError}
          nftsListIsFetching={nftsListIsFetching}
          nftsListError={nftsListError as ApiError | null}
          nftsListData={nftsListData}
          totalCount={totalCount}
          page={nftPage}
          rowsPerPage={nftRowsPerPage}
          onPageChange={(event, newPage) => handleChangePage(event, newPage, setNftPage)}
          onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setNftRowsPerPage, setNftPage)}
        />
      </TabPanel>
    </Box>
  );
}

export default Assets;
