//  Copyright 2022. The Tari Project
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

import { Alert, Divider } from "@mui/material";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import { useTheme } from "@mui/material/styles";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import {
  ConfidentialViewVaultBalanceRequest,
  ConfidentialViewVaultBalanceResponse,
} from "@tari-project/ootle-ts-bindings";
import { confidentialViewVaultBalance } from "@utils/json_rpc";
import { useState } from "react";
import { formatDecryptedBalance, validateViewVaultForm } from "./decryptBalanceHelpers";

import SearchedRange from "./SearchedRange";

function ViewVaultBalanceForm() {
  const [formState, setFormState] = useState({
    vaultId: "",
    keyId: "",
  });
  const [vaultBalance, setVaultBalance] = useState<ConfidentialViewVaultBalanceResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  const onViewBalanceClicked = async () => {
    const validated = validateViewVaultForm(formState);
    if (!validated.ok) {
      setError(validated.error);
      return;
    }

    setError(null);
    setIsLoading(true);
    try {
      const resp = await confidentialViewVaultBalance({
        vault_id: validated.vaultId,
        minimum_expected_value: null,
        maximum_expected_value: null,
        view_key_id: validated.viewKeyId,
      } as ConfidentialViewVaultBalanceRequest);

      setVaultBalance(resp);
    } catch (e: unknown) {
      setVaultBalance(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const searched = vaultBalance && <SearchedRange searched={vaultBalance.searched} />;

  const balances =
    vaultBalance &&
    Object.keys(vaultBalance.balances).map((key) => {
      return (
        <Box key={key}>
          <Typography>
            {key}: {formatDecryptedBalance(vaultBalance.balances[key])}
          </Typography>
        </Box>
      );
    });

  const onChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setError(null);
    setFormState({
      ...formState,
      [e.target.name]: e.target.value,
    });
  };

  return (
    <>
      <Box className="flex-container" sx={{ marginBottom: 4 }}>
        <TextField
          name="keyId"
          label="Key ID"
          value={formState.keyId}
          onChange={onChange}
          style={{ flexGrow: 1 }}
          slotProps={{ htmlInput: { inputMode: "numeric" } }}
        />
        <TextField
          name="vaultId"
          label="Vault Id"
          value={formState.vaultId}
          onChange={onChange}
          style={{ flexGrow: 1 }}
        />
        <Button
          variant="contained"
          onClick={onViewBalanceClicked}
          disabled={isLoading || !formState.vaultId.trim() || !formState.keyId.trim()}
        >
          {isLoading ? "Fetching..." : "Fetch Balance"}
        </Button>
      </Box>
      {error && (
        <Alert severity="error" sx={{ marginBottom: 2 }}>
          {error}
        </Alert>
      )}
      {searched}
      {balances && (
        <>
          <Typography variant="h3">Balances</Typography>
          {balances}
        </>
      )}
    </>
  );
}

function ViewVaultBalance() {
  const theme = useTheme();
  return (
    <Box
      style={{
        display: "flex",
        flexDirection: "column",
        gap: theme.spacing(3),
        paddingTop: theme.spacing(3),
      }}
    >
      <p>
        Brute force an external vault balance using a secret view key. This applies to resources that have the view key
        enabled.
      </p>
      <Box>
        <ViewVaultBalanceForm />
      </Box>
      <Divider />
    </Box>
  );
}

export default ViewVaultBalance;
