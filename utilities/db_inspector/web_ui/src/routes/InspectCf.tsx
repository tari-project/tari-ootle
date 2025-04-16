//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { Box, Button, Divider, Grid, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { client } from "../store/databases.ts";
import { Link as RouterLink, useParams } from "react-router-dom";
import { DataGrid, GridColDef } from "@mui/x-data-grid";
import { useEffect, useState } from "react";
import { Refresh } from "@mui/icons-material";


type Row = Record<string, string | object>;

interface Column {
  field: string;
  label: string;
}

interface ColumnFamilyData {
  columns: Column[];
  rows: Row[];
}

function estimateWidth(labelLength: number, data: Row[], getter: (row: Row) => any): number {
  return Math.min(2000, data.reduce((acc, row) => {
    const value = getter(row);
    if (typeof value === "string") {
      return Math.max(acc, value.length * 8);
    }

    return Math.max(acc, value.toString().length * 8);
  }, labelLength * 15));
}

function generateColumns(data: ColumnFamilyData): GridColDef<Row>[] {
  if (data.columns.length === 0) {
    if (data.rows.length === 0) {
      return [];
    }
    // Use the depth 1 fields
    return Object.keys(data.rows[0]).map((key) => {
      const getter = valueGetter(key);
      return {
        field: key,
        headerName: key,
        valueGetter: (_, val) => getter(val),
        // Try roughly estimating the width of the column based on the length of the data
        width: estimateWidth(key.length, data.rows, getter),
      };
    });
  }

  const cols = data.columns.map((col) => {
    const getter = valueGetter(col.field);
    const labelLength = col.label.length;
    return {
      field: col.field,
      headerName: col.label,
      valueGetter: (_, val) => getter(val),
      // Try roughly estimating the width of the column based on the length of the data
      width: estimateWidth(labelLength, data.rows, getter),
    } as GridColDef<Row>;
  });

  return [{
    field: "id",
    headerName: "Key",
  }, ...cols];
}

export default function InspectCf() {
  const theme = useTheme();
  const { dbName, cfName } = useParams();
  const [data, setData] = useState<ColumnFamilyData | null>(null);
  const [error, setError] = useState<string | null>(null);

  const fetch = () => {
    client.listCfItems(dbName!, cfName!, { limit: 500 }).then((res) => {
      setData(res);
    }).catch((err) => {
      setError(err.message);
    });
  };

  useEffect(fetch, [dbName, cfName]);

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


  const columns = generateColumns(data);

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
            {/*  Refresh button*/}
            <Button
              variant="outlined"
              color="primary"
              onClick={() => fetch()}
              style={{ marginLeft: theme.spacing(2) }}
            >
              <Refresh />
            </Button>
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