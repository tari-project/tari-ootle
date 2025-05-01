//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { Box, Button, Divider, Grid, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { useDatabaseCfsList } from "../store/databases.ts";
import { Link as RouterLink, useParams } from "react-router-dom";
import { DataGrid, GridSortModel } from "@mui/x-data-grid";
import { useState } from "react";
import prettyBytes from "pretty-bytes";

export default function ListColumnFamilies() {
  const theme = useTheme();
  const { dbName } = useParams();
  const [selectedCf, setSelectedCf] = useState<string | number | null>(null);
  const [sortModel, setSortModel] = useState<GridSortModel>([
    { field: "name", sort: "asc" },
  ]);

  const { data: cfs, isLoading } = useDatabaseCfsList(dbName || "<NOTHING>");

  if (isLoading || !cfs) {
    return <div>Loading...</div>;
  }

  const cols = [
    {
      field: "name",
      headerName: "Name",
      width: 200,
    },
    {
      field: "num_entries",
      headerName: "# Entries",
      width: 200,
    },
    {
      field: "total_entries_bytes",
      headerName: "Total Bytes",
      width: 400,
      valueFormatter: (_n: unknown, value: any) => {
        const bytes = value.total_entries_bytes;
        if (value.num_entries === 0) {
          return `${prettyBytes(bytes)} (avg: --)`;
        }
        return `${prettyBytes(bytes)} (avg: ${prettyBytes(bytes / value.num_entries)})`;
      },
    },
  ];

  const onSelectedRowChange = (selection: string | number) => {
    setSelectedCf(selection);
  };

  return (
    <>
      <Grid size={{ xs: 12, md: 12, lg: 12 }}>
        <Box
          className="flex-container"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "left",
          }}
        >
          <Typography
            variant="h4"
            style={{ paddingBottom: theme.spacing(2) }}
          >
            Select Column Family
            <Button
              style={{ margin: theme.spacing(2) }}
              variant="contained"
              color="primary"
              component={RouterLink}
              to={`/databases/${dbName}/column-families/${selectedCf}`}
              disabled={!selectedCf}
            >Inspect</Button>
          </Typography>
        </Box>
        <Divider />
      </Grid>

      <Grid container spacing={3}>
        <DataGrid
          rows={cfs}
          columns={cols}
          getRowId={(row) => row.name}
          initialState={{
            pagination: {
              paginationModel: {
                pageSize: 20,
              },
            },
          }}
          sortModel={sortModel}
          onSortModelChange={setSortModel}
          sortingOrder={["desc", "asc", null]}
          disableMultipleRowSelection
          onRowSelectionModelChange={(selections) => {
            onSelectedRowChange(selections[0]);
          }}
          checkboxSelection
        />
      </Grid>
    </>
  );
}