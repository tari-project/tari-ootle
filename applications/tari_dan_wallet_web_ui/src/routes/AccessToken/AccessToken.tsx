// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import useAuthStore from "../../store/authStore";
import Grid from "@mui/material/Grid";

function AccessToken() {
    const {authToken} = useAuthStore();

    return (
        <>
            <Grid item xs={12} md={12} lg={12}>
                {authToken}
            </Grid>
        </>
    );
}

export default AccessToken;