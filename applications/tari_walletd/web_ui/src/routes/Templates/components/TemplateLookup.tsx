// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause
import { useTemplateGet } from "@api/hooks/useTemplate";
import { Alert, Box, Button, Collapse, IconButton, InputAdornment, Table, TableBody, TableContainer, TableHead, TableRow, TextField } from "@mui/material";
import ClearIcon from "@mui/icons-material/Clear";
import FunctionItem from "@routes/Templates/components/FunctionItem";
import { NestedCell } from "@routes/Templates/components/StyledTableComponents";
import { useState } from "react";

export default function TemplateLookup() {
  const [input, setInput] = useState("");
  const [address, setAddress] = useState("");

  const templateAddress = address.startsWith("template_") ? address.slice("template_".length) : address;

  const { data, isLoading, isError, error } = useTemplateGet(
    { template_address: templateAddress },
    { enabled: !!templateAddress },
  );

  const functions = data?.template_definition?.V1?.functions || [];

  function handleLookup() {
    const trimmed = input.trim();
    if (trimmed) {
      setAddress(trimmed);
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
                  <IconButton size="small" onClick={() => { setInput(""); setAddress(""); }}>
                    <ClearIcon fontSize="small" />
                  </IconButton>
                </InputAdornment>
              ) : null,
            },
          }}
        />
        <Button variant="contained" onClick={handleLookup} disabled={!input.trim() || isLoading}>
          {isLoading ? "Loading..." : "Fetch"}
        </Button>
      </Box>

      {isError && (
        <Alert severity="error" sx={{ mt: 1 }}>
          {(error as Error)?.message || "Failed to fetch template"}
        </Alert>
      )}

      <Collapse in={functions.length > 0} timeout="auto">
        <h3>Functions</h3>
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
