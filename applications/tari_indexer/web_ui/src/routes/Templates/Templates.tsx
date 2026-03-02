//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  IconButton,
  InputAdornment,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import React, { useState, useCallback, useEffect } from "react";
import { useSearchParams } from "react-router-dom";
import CopyToClipboard from "../../Components/CopyToClipboard";
import { useGetTemplateDefinition } from "../../api/hooks/useTemplates";
import type { FunctionDef, Type } from "@tari-project/ootle-ts-bindings";
import SearchIcon from "@mui/icons-material/Search";
import ClearIcon from "@mui/icons-material/Clear";

function validateTemplateAddress(id: string): string | null {
  if (!id) return null;
  if (!/^[a-fA-F0-9]{64}$/.test(id)) {
    return "Template address must be a 64-character hex string.";
  }
  return null;
}

function formatType(type: Type): string {
  if (typeof type === "string") {
    return type;
  }
  if ("Vec" in type) {
    return `Vec<${formatType((type as { Vec: Type }).Vec)}>`;
  }
  if ("Option" in type) {
    return `Option<${formatType((type as { Option: Type }).Option)}>`;
  }
  if ("Tuple" in type) {
    return `(${(type as { Tuple: Type[] }).Tuple.map(formatType).join(", ")})`;
  }
  if ("Other" in type) {
    return (type as { Other: { name: string } }).Other.name;
  }
  return JSON.stringify(type);
}

function formatSelfParam(func: FunctionDef): string {
  const hasSelf = func.arguments.some((arg) => arg.name === "self");
  if (!hasSelf) return "";
  return func.is_mut ? "&mut self" : "&self";
}

function formatSignature(func: FunctionDef, isMethod: boolean): React.ReactNode {
  const selfParam = isMethod ? formatSelfParam(func) : "";
  const visibleArgs = func.arguments.filter((arg) => arg.name !== "self");
  const argParts = visibleArgs.map((a) => `${a.name}: ${formatType(a.arg_type)}`);
  if (selfParam) argParts.unshift(selfParam);
  const params = argParts.join(", ");
  const output = formatType(func.output);

  return (
    <>
      <span style={{ color: "#7c3aed" }}>pub fn </span>
      <strong>{func.name}</strong>
      ({params})
      {output !== "Unit" && <> &rarr; {output}</>}
    </>
  );
}

function FunctionList({ functions, isMethod }: { functions: FunctionDef[]; isMethod: boolean }) {
  return (
    <Stack spacing={0.5}>
      {functions.map((func) => (
        <Box
          key={func.name}
          sx={{
            fontFamily: "'Courier New', Courier, monospace",
            fontSize: "14px",
            py: 0.5,
            px: 1,
          }}
        >
          {formatSignature(func, isMethod)}
        </Box>
      ))}
    </Stack>
  );
}

function TemplateDetails({ data }: { data: any }) {
  // The response has { name, definition, code_size } from the REST API,
  // but the JS client types it as { template_definition: TemplateDef }.
  // Handle both shapes.
  const definition = data.definition || data.template_definition;
  const name = data.name || definition?.V1?.template_name;
  const codeSize = data.code_size;

  if (!definition) {
    return <Alert severity="warning">No template definition found in response.</Alert>;
  }

  const defV1 = definition.V1;
  if (!defV1) {
    return <Alert severity="warning">Unsupported template definition version.</Alert>;
  }

  const allFunctions = defV1.functions || [];
  const isSelfParam = (arg: { name: string }) => arg.name === "self";
  const methods = allFunctions.filter((f: FunctionDef) => f.arguments.some(isSelfParam));
  const functions = allFunctions.filter((f: FunctionDef) => !f.arguments.some(isSelfParam));

  return (
    <Stack spacing={3}>
      <Box sx={{ display: "flex", alignItems: "center", gap: 2, flexWrap: "wrap" }}>
        <Typography variant="h6">{name}</Typography>
        {codeSize != null && (
          <Chip
            label={`${(codeSize / 1024).toFixed(1)} KB`}
            size="small"
            variant="outlined"
          />
        )}
        <Chip
          label={`ABI v${defV1.abi_version}`}
          size="small"
          variant="outlined"
        />
      </Box>

      <Divider />

      {functions.length > 0 && (
        <>
          <Typography variant="subtitle2">Functions ({functions.length})</Typography>
          <FunctionList functions={functions} isMethod={false} />
        </>
      )}

      {methods.length > 0 && (
        <>
          <Typography variant="subtitle2">Methods ({methods.length})</Typography>
          <FunctionList functions={methods} isMethod={true} />
        </>
      )}
    </Stack>
  );
}

function TemplatesLayout() {
  const [searchParams, setSearchParams] = useSearchParams();
  const initialAddress = searchParams.get("address") || "";
  const [addressInput, setAddressInput] = useState(initialAddress);
  const [fetchAddress, setFetchAddress] = useState<string | null>(initialAddress || null);
  const [validationError, setValidationError] = useState<string | null>(null);

  const { data, isLoading, isError, error } = useGetTemplateDefinition({
    address: fetchAddress,
    enabled: !!fetchAddress,
  });

  useEffect(() => {
    const addr = searchParams.get("address") || "";
    if (addr && addr !== fetchAddress) {
      setAddressInput(addr);
      setValidationError(null);
      setFetchAddress(addr);
    }
  }, [searchParams]);

  const handleFetch = useCallback(() => {
    const trimmed = addressInput.trim();
    if (!trimmed) return;
    const error = validateTemplateAddress(trimmed);
    if (error) {
      setValidationError(error);
      return;
    }
    setValidationError(null);
    setFetchAddress(trimmed);
    setSearchParams({ address: trimmed });
  }, [addressInput, setSearchParams]);

  const handleClear = useCallback(() => {
    setAddressInput("");
    setFetchAddress(null);
    setValidationError(null);
    setSearchParams({});
  }, [setSearchParams]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        handleFetch();
      }
    },
    [handleFetch],
  );

  return (
    <>
      <Grid size={12}>
        <PageHeading>Templates</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <Stack spacing={3}>
            <Typography variant="body2" color="text.secondary">
              Look up a template definition by its address (64-character hex hash).
            </Typography>
            <Box sx={{ display: "flex", gap: 2 }}>
              <TextField
                fullWidth
                label="Template Address"
                placeholder="0000000000000000000000000000000000000000000000000000000000000000"
                value={addressInput}
                onChange={(e) => {
                  setAddressInput(e.target.value);
                  if (validationError) setValidationError(null);
                }}
                onKeyDown={handleKeyDown}
                size="medium"
                error={!!validationError}
                helperText={validationError}
                sx={{ fontFamily: "'Courier New', Courier, monospace" }}
                slotProps={{
                  input: {
                    endAdornment: addressInput ? (
                      <InputAdornment position="end">
                        <IconButton onClick={handleClear} edge="end" size="small">
                          <ClearIcon fontSize="small" />
                        </IconButton>
                      </InputAdornment>
                    ) : null,
                  },
                }}
              />
              <Button
                variant="contained"
                onClick={handleFetch}
                disabled={!addressInput.trim() || isLoading}
                startIcon={isLoading ? <CircularProgress size={20} /> : <SearchIcon />}
                sx={{ minWidth: "120px" }}
              >
                {isLoading ? "Fetching" : "Fetch"}
              </Button>
            </Box>

            {fetchAddress && (
              <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                <Typography variant="body2" color="text.secondary" sx={{ fontFamily: "'Courier New', Courier, monospace", wordBreak: "break-all" }}>
                  {fetchAddress}
                </Typography>
                <CopyToClipboard copy={fetchAddress} />
              </Box>
            )}

            {isError && (
              <Alert severity="error">
                {(error as any)?.message || "Failed to fetch template. Check the address and try again."}
              </Alert>
            )}

            {data && (
              <>
                <Divider />
                <TemplateDetails data={data} />
              </>
            )}
          </Stack>
        </StyledPaper>
      </Grid>
    </>
  );
}

export default TemplatesLayout;
