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

import Typography from "@mui/material/Typography";
import TextField from "@mui/material/TextField";
import { useState } from "react";
import Button from "@mui/material/Button";
import Box from "@mui/material/Box";
import { useTheme } from "@mui/material/styles";
import { Divider } from "@mui/material";
import { stealthDecryptUtxoBalance } from "@utils/json_rpc";
import { StealthUtxosDecryptValueRequest, StealthUtxosDecryptValueResponse } from "@tari-project/typescript-bindings";

function DecryptUtxoBalanceForm() {
  const [formState, setFormState] = useState({
    resourceAddress: null,
    utxoId: null,
    minimumExpectedValue: null,
    maximumExpectedValue: 100000000,
    keyId: 0,
  });
  const [balance, setBalance] = useState<StealthUtxosDecryptValueResponse | null>(null);

  const onViewBalanceClicked = async () => {
    const resp = await stealthDecryptUtxoBalance({
      resource_address: formState.resourceAddress!,
      ids: [formState.utxoId!],
      minimum_expected_value: formState.minimumExpectedValue ? BigInt(formState.minimumExpectedValue) : null,
      maximum_expected_value: BigInt(formState.maximumExpectedValue),
      view_key_id: BigInt(formState.keyId),
    } as StealthUtxosDecryptValueRequest);

    setBalance(resp);
  };

  const balances =
    balance &&
    Object.keys(balance?.values).map((key) => {
      return (
        <Box key={key}>
          <Typography>
            {key}: {balance.values[key]?.toString() || "Failed not decrypt value"}
          </Typography>
        </Box>
      );
    });

  const onChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setFormState({
      ...formState,
      [e.target.name]: e.target.value,
    });
  };

  return (
    <>
      <Box className="flex-container" sx={{ marginBottom: 4 }}>
        <TextField name="keyId" label="Key ID" value={formState.keyId} onChange={onChange} style={{ flexGrow: 1 }} />
        <TextField
          name="resourceAddress"
          label="Resource Address"
          value={formState.resourceAddress}
          onChange={onChange}
          style={{ flexGrow: 1 }}
        />
        <TextField name="utxoId" label="UTXO ID" value={formState.utxoId} onChange={onChange} style={{ flexGrow: 1 }} />
        <TextField
          name="minimumExpectedValue"
          label="Minimum Expected Value"
          value={formState.minimumExpectedValue}
          onChange={onChange}
          style={{ flexGrow: 1 }}
        />
        <TextField
          name="maximumExpectedValue"
          label="Maximum Expected Value"
          value={formState.maximumExpectedValue}
          onChange={onChange}
          style={{ flexGrow: 1 }}
        />

        <Button variant="contained" onClick={onViewBalanceClicked} disabled={!formState.resourceAddress}>
          Decrypt
        </Button>
      </Box>
      {balances && (
        <>
          <Typography variant="h3">Balances</Typography>
          {balances}
        </>
      )}
    </>
  );
}

function DecryptUtxoBalance() {
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
        Brute force a UTXO balance using a secret view key. This applies to resources that have the view key enabled.
      </p>
      <Box>
        <DecryptUtxoBalanceForm />
      </Box>
      <Divider />
    </Box>
  );
}

export default DecryptUtxoBalance;
