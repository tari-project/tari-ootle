//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { Box, Button, Divider, Grid, TextField, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { client } from "../store/databases.ts";
import { Link as RouterLink, useParams } from "react-router-dom";
import { DataGrid, GridColDef, GridPaginationModel } from "@mui/x-data-grid";
import { useEffect, useMemo, useRef, useState } from "react";
import { Refresh } from "@mui/icons-material";
import { Params } from "../client.ts";


type Row = Record<string, string | object>;

interface Column {
  field: string;
  label: string;
}

interface ColumnFamilyData {
  columns: Column[];
  rows: Row[];
  total_entries: number;
}

interface PaginationModel extends GridPaginationModel {
  query?: string;
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
  const [pagination, setPagination] = useState<PaginationModel>({ page: 0, pageSize: 20 });
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const fetch = () => {
    setError(null);
    setIsLoading(true);

    const query = { limit: pagination.pageSize, page: pagination.page, query_prefix_hex: pagination.query || "" };
    client.listCfItems(dbName!, cfName!, query as Params).then((res) => {
      setData(res);
    }).catch((err) => {
      setError(err.message);
    }).finally(() => {
      setIsLoading(false);
    });
  };

  useEffect(fetch, [dbName, cfName, pagination]);

  const columns = data ? generateColumns(data) : [];
  // Following lines are here to prevent `rowCount` from being undefined during the loading
  const rowCountRef = useRef(data?.total_entries || 0);

  const rowCount = useMemo(() => {
    if (data?.total_entries !== undefined) {
      rowCountRef.current = data.total_entries;
    }
    return rowCountRef.current;
  }, [data?.total_entries]);


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
            <Button
              variant="outlined"
              color="primary"
              onClick={() => fetch()}
              style={{ marginLeft: theme.spacing(2) }}
            >
              <Refresh />
            </Button>
            <TextField
              variant="outlined"
              size="small"
              label="Prefix key query"
              style={{ marginLeft: theme.spacing(2), width: "600px" }}
              onChange={(e) => {
                const hex = e.target.value;
                // check if valid hex
                if (hex && !/^[0-9a-fA-F]*$/.test(hex)) {
                  setError("Invalid hex string");
                  return;
                }
                setPagination({ ...pagination, page: 0, query: e.target.value });
              }}
              value={pagination.query || ""}
              placeholder="Enter a key prefix in hex"
            />
            <Button
              variant="outlined"
              color="secondary"
              onClick={() => setPagination({ ...pagination, page: 0, query: "" })}
              style={{ marginLeft: theme.spacing(2) }}
              disabled={!pagination.query}
            >
              Clear
            </Button>
          </Typography>

        </Box>
        <Divider />
        {error && (
          <Box
            className="flex-container"
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
            }}
          >
            <Typography variant="h6" color="error">
              Error: {error}
            </Typography>
          </Box>
        )}
      </Grid>

      <Grid container spacing={3}>
        <DataGrid
          rows={data?.rows || []}
          columns={columns}
          loading={isLoading}
          rowCount={rowCount}
          initialState={{
            pagination: {
              paginationModel: {
                pageSize: pagination.pageSize,
              },
            },
          }}
          paginationMode="server"
          onPaginationModelChange={(p) => setPagination({ ...pagination, ...p })}
          sortingOrder={["desc", "asc", null]}
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
    if (value === undefined || value === null) {
      return field + " null";
    }
    for (const part of parts) {
      value = value[part];
      if (value === undefined) {
        return field + " undefined";
      }
    }

    if (typeof value === "object") {
      return JSON.stringify(value, null, 2);
    }
    return value;
  };
}