// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause
import { useTemplateGet } from "@api/hooks/useTemplate";
import ClearIcon from "@mui/icons-material/Clear";
import {
  Alert,
  Box,
  Button,
  Collapse,
  IconButton,
  InputAdornment,
  Table,
  TableBody,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Typography,
} from "@mui/material";
import FunctionItem from "@routes/Templates/components/FunctionItem";
import { NestedCell } from "@routes/Templates/components/StyledTableComponents";
import { useState } from "react";
import { normalizeTemplateAddress, resolveTemplateLookupAction } from "./templateLookupHelpers";

export default function TemplateLookup() {
  const [input, setInput] = useState("");
  const [address, setAddress] = useState("");

  const normalizedInput = normalizeTemplateAddress(input);

  const { data, isFetching, isError, error, refetch } = useTemplateGet(
    { template_address: address },
    { enabled: !!address },
  );

  const functions = data?.template_definition?.V2?.functions || [];

  function handleLookup() {
    if (isFetching) {
      return;
    }
    const action = resolveTemplateLookupAction(input, address);
    if (action.kind === "set-address") {
      setAddress(action.address);
    } else if (action.kind === "refetch") {
      void refetch();
    }
  }

  return (
    <Box>
      <Box sx={{ display: "flex", gap: 1, alignItems: "flex-start" }}>
        <TextField
          label="Template Address"
          placeholder="template_abcd... or abcd..."
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleLookup();
          }}
          size="small"
          fullWidth
          slotProps={{
            input: {
              endAdornment: input ? (
                <InputAdornment position="end">
                  <IconButton
                    size="small"
                    onClick={() => {
                      setInput("");
                      setAddress("");
                    }}
                  >
                    <ClearIcon fontSize="small" />
                  </IconButton>
                </InputAdornment>
              ) : null,
            },
          }}
        />
        <Button variant="contained" onClick={handleLookup} disabled={!normalizedInput || isFetching}>
          {isFetching ? "Loading..." : "Fetch"}
        </Button>
      </Box>

      {isError && (
        <Alert severity="error" sx={{ mt: 1 }}>
          {(error as Error)?.message || "Failed to fetch template"}
        </Alert>
      )}

      <Collapse in={functions.length > 0} timeout="auto">
        <Typography variant="h6" component="h3" sx={{ mt: 2 }}>
          Functions
        </Typography>
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow>
                <NestedCell>Name</NestedCell>
                <NestedCell>Arguments</NestedCell>
                <NestedCell align="right">Output Type</NestedCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {functions.map((fn) => (
                <FunctionItem key={`lookup_fn_${fn.name}`} functionDef={fn} />
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      </Collapse>
    </Box>
  );
}
