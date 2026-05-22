//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import PageHeading from "@components/PageHeading";
import Grid from "@mui/material/Grid";
import ApiKeys from "@routes/Wallet/Components/ApiKeys";

/// Page wrapper that mounts the API keys management table at
/// `/api-keys`. Mirrors the AccessTokens layout pattern: a heading row
/// + the component itself. The underlying `ApiKeys` component renders
/// its own paper/alerts, so this wrapper doesn't add one.
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
