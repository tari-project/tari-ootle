// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import PageHeading from "@components/PageHeading";
import { Stack } from "@mui/material";
import Grid from "@mui/material/Grid";
import TemplateList from "./components/TemplateList";
import Wrapper from "./components/Wrapper";

function Templates() {
  return (
    <>
      <Grid size={12}>
        <PageHeading>Templates</PageHeading>
      </Grid>
      <Wrapper>
        <Stack spacing={1}>
          <TemplateList />
        </Stack>
      </Wrapper>
    </>
  );
}

export default Templates;
