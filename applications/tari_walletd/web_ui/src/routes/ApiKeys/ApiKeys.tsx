//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import PageHeading from "@components/PageHeading";
import Grid from "@mui/material/Grid";
import ApiKeys from "@routes/Wallet/Components/ApiKeys";

function ApiKeysLayout() {
  return (
    <>
      <Grid size={12}>
        <PageHeading>API Keys</PageHeading>
      </Grid>
      <Grid size={12}>
        <ApiKeys />
      </Grid>
    </>
  );
}

export default ApiKeysLayout;
