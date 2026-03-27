// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import PageHeading from "@components/PageHeading";
import { StyledPaper } from "@components/StyledComponents";
import { Stack, Typography } from "@mui/material";
import Grid from "@mui/material/Grid";
import TemplateList from "./components/TemplateList";
import TemplateLookup from "./components/TemplateLookup";
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
      <Grid size={12}>
        <StyledPaper>
          <Typography variant="h6" gutterBottom>
            Lookup Template
          </Typography>
          <TemplateLookup />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default Templates;
