//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { Box, Button, Divider, Grid, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { client } from "../store/databases.ts";
import { Link as RouterLink, useParams } from "react-router-dom";
import { DataGrid, GridColDef, GridRowModel } from "@mui/x-data-grid";
import { useEffect, useState } from "react";


type Row = Record<string, string | object>;

interface Column {
  field: string;
  label: string;
}

interface ColumnFamilyData {
  columns: Column[];
  rows: Row[];
}

export default function InspectCf() {
  const theme = useTheme();
  const { dbName, cfName } = useParams();
  const [data, setData] = useState<ColumnFamilyData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    client.listCfItems(dbName!, cfName!, { limit: 1000 }).then((res) => {
      setData(res);
    }).catch((err) => {
      setError(err.message);
    });

  }, []);

  if (error) {
    return (
      <>
        <Grid size={{ xs: 12, md: 12, lg: 12 }}>
          <Box
            className="flex-container"
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
            }}
          >
            <Typography
              variant="h4"
              style={{
                paddingBottom: theme.spacing(2),
              }}
            >
              <RouterLink to={`/databases/${dbName}`}>{dbName}</RouterLink> - {cfName}
            </Typography>
          </Box>
          <Divider />
        </Grid>
        <Typography variant="h6" color="error">
          Error: {error}
        </Typography>
      </>
    );
  }

  if (!data) {
    return <div>Loading...</div>;
  }


  const cols = data.columns.map((col) => {
    const getter = valueGetter(col.field);
    const labelLength = col.label.length;
    return {
      field: col.field,
      headerName: col.label,
      valueGetter: (_, val) => getter(val),
      width: Object.keys(data.rows).reduce((acc, _k, i) => {
        const value = getter(data.rows[i]);
        if (typeof value === "string") {
          return Math.max(acc, value.length + 100);
        }

        return Math.max(acc, value.toString().length + 100);
      }, labelLength * 10),
    } as GridColDef<(typeof data.rows)[number]>;
  });

  const columns = [{
    field: "id",
    headerName: "Key",
  }, ...cols];

  return (
    <>
      <Grid size={{ xs: 12, md: 12, lg: 12 }}>
        <Box
          className="flex-container"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <Typography
            variant="h4"
            style={{
              paddingBottom: theme.spacing(2),
            }}
          >
            <RouterLink to={`/databases/${dbName}`}>{dbName}</RouterLink> - {cfName}
          </Typography>
        </Box>
        <Divider />
      </Grid>

      <Grid container spacing={3}>
        <DataGrid
          rows={data.rows}
          columns={columns}
          initialState={{
            pagination: {
              paginationModel: {
                pageSize: 20,
              },
            },
          }}
          sortingOrder={["desc", "asc", null]}
          disableMultipleRowSelection
          checkboxSelection
        />
      </Grid>
    </>
  );
}

function valueGetter(field: string) {
  return (row: any) => {
    const parts = field.split(".");
    let value = row;
    for (const part of parts) {
      value = value[part];
      if (value === undefined) {
        return "Error: invalid value at path " + field;
      }
    }

    if (typeof value === "object") {
      return JSON.stringify(value, null, 2);
    }
    return value;
  };
}