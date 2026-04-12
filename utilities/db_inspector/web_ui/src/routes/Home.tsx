//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { Box, Card, CardActionArea, CardContent, Divider, Grid, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { useDatabasesList } from "../store/databases.ts";
import { Link as RouterLink } from "react-router-dom";

export default function Home() {
  const theme = useTheme();

  const { data: databases, isLoading, error } = useDatabasesList();

  if (error) {
    return (
      <div>
        <p>Error loading databases: {error.message}</p>
      </div>
    );
  }

  if (isLoading) {
    return <div>Loading...</div>;
  }

  return (
    <>
      <Grid size={{ xs: 12, md: 12, lg: 12 }}>
        <Box
          className="flex-container"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            width: "100%",
          }}
        >
          <Typography
            variant="h4"
            style={{
              paddingBottom: theme.spacing(2),
              width: "100%",
            }}
          >
            Select Database
          </Typography>
          {/*<ActionMenu />*/}
        </Box>
        <Divider />
      </Grid>

      <Grid container spacing={3}>
        {databases?.map((db, i) => (
          <Grid size={{ xs: 12, sm: 6, md: 4, lg: 3 }} key={i}>
            <Card elevation={3}>
              <CardActionArea component={RouterLink} to={"/databases/" + encodeURIComponent(db.name)}>
                <CardContent>
                  <Typography variant="h6" component="div">
                    {db.name} ({db.path})
                  </Typography>
                </CardContent>
              </CardActionArea>
            </Card>
          </Grid>
        ))}
      </Grid>
    </>
  );
}