// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import useAuthStore from "../../store/authStore";
import Grid from "@mui/material/Grid";

function AccessToken() {
    const {authToken} = useAuthStore();

    return (
        <>
            <Grid container xs={6} md={6} lg={6}>
                <Grid item xs={6} md={6} lg={6}>
                    {authToken}
                </Grid>
            </Grid>
        </>
    );
}

export default AccessToken;