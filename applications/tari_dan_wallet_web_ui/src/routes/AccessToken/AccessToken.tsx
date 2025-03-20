// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import useAuthStore from "../../store/authStore";
import Grid from "@mui/material/Grid";
import CopyToClipboard from "../../Components/CopyToClipboard";
import Typography from "@mui/material/Typography";
import TextField from "@mui/material/TextField/TextField";

function AccessToken() {
  const { authToken } = useAuthStore();

  return (
    <>
      <Grid container alignItems="center" justifyContent="center">
        <Grid item xs={11} sm={11} md={11} lg={11}>
          <Typography>Your token:</Typography>
          <TextField disabled multiline={true} value={authToken} style={{ width: "100%", height: "10vh" }} />
        </Grid>
        <Grid item xs={1} sm={1} md={1} lg={1}>
          <CopyToClipboard title="Copy token" copy={authToken} iconWidth="25px" iconHeight="25px" />
        </Grid>
      </Grid>
    </>
  );
}

export default AccessToken;
