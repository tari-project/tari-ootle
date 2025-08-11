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

import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import { BsTextarea } from "react-icons/bs";
import { TextareaAutosize } from "@mui/material";
import { useState } from "react";
import useManifestCodeStore from "../../store/manifestStore";
import Button from "@mui/material/Button/Button";
import TextField from "@mui/material/TextField/TextField";
import Box from "@mui/material/Box";
import { useSubmitManifest } from "../../api/hooks/useTransactions";
import { rejectReasonToString } from "@tari-project/typescript-bindings";

function Manifest() {
  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        <PageHeading>Manifest</PageHeading>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <ManifestEditor />
        </StyledPaper>
      </Grid>
    </>
  );
}

function ManifestEditor() {
  const manifest = useManifestCodeStore();
  const [fee, setFee] = useState<bigint>(0n);
  const [finalizeError, setFinalizeError] = useState<string | null>(null);

  const { mutateAsync: submitManifest, isLoading: isSubmittingManifest, error } = useSubmitManifest();

  const isDryRun = !fee;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    console.log("Manifest code submitted:", manifest.code);
    console.log("Fee submitted:", fee);
    submitManifest({
      manifest: manifest.code,
      variables: manifest.variables,
      max_fee: isDryRun ? 3000 : Number(fee),
      signing_key_index: null,
      dry_run: isDryRun,
    })
      .then((response) => {
        if (!isDryRun) {
          console.log("Manifest submitted successfully:", response);
          return;
        }
        const finalize = response.result?.finalize;
        if (!finalize && isDryRun) {
          throw new Error("No result returned for dry run");
        }
        if ("Accept" in finalize!.result) {
          setFee(BigInt(finalize!.fee_receipt.total_fees_paid) + 100n);
          console.log("Dry run successful:", finalize);
        } else if ("Reject" in finalize!.result) {
          setFinalizeError(rejectReasonToString(finalize!.result.Reject));
        } else if ("AcceptFeeRejectRest" in finalize!.result) {
          setFinalizeError(rejectReasonToString(finalize!.result.AcceptFeeRejectRest[1]));
        }
      })
      .catch((error) => {
        console.error("Error submitting manifest:", error);
      });
  };

  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        {error && (
          <Box sx={{ color: "red" }}>
            <p>{error.message}</p>
          </Box>
        )}
        {finalizeError && (
          <Box sx={{ color: "red" }}>
            <p>{finalizeError}</p>
          </Box>
        )}

        <form onSubmit={handleSubmit}>
          <TextareaAutosize
            minRows={40}
            aria-label="Manifest code editor"
            name="manifest-code"
            value={manifest.code}
            onChange={(e) => manifest.setCode(e.target.value)}
            style={{ width: 1000 }}
          />
          <Box className="flex-container" style={{ justifyContent: "flex-start" }}>
            <VariableEditor
              variables={manifest.variables}
              onAdd={manifest.addVariable}
              onRemove={manifest.removeVariable}
            />
          </Box>
          <Box className="flex-container" style={{ justifyContent: "flex-start" }}>
            <TextField
              name="max-fee"
              placeholder="Max fee"
              value={fee}
              onChange={(e) => setFee(BigInt(e.target.value))}
              type="number"
            />
            <Button type="submit" variant="contained" color="primary">
              {isSubmittingManifest ? "Submitting..." : fee ? "Submit" : "Estimate fee"}
            </Button>
          </Box>
        </form>
      </Grid>
    </>
  );
}

function VariableEditor({
  variables,
  onAdd,
  onRemove,
}: {
  variables: Record<string, string>;
  onAdd: (key: string, value: string) => void;
  onRemove: (key: string) => void;
}) {
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");

  return (
    <Grid item xs={12} md={12} lg={12}>
      <TextField name="variable-key" placeholder="Variable key" value={key} onChange={(e) => setKey(e.target.value)} />
      <TextField
        name="variable-value"
        placeholder="Variable value"
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <Button
        variant="contained"
        color="primary"
        onClick={() => {
          onAdd(key, value);
          setKey("");
          setValue("");
        }}
      >
        Add Variable
      </Button>
      {Object.entries(variables).map(([k, v]) => (
        <Box key={k} className="flex-container" style={{ justifyContent: "flex-start" }}>
          <span>{`${k}: ${v}`}</span>
          <Button variant="outlined" color="secondary" onClick={() => onRemove(k)}>
            Remove
          </Button>
        </Box>
      ))}
    </Grid>
  );
}

export default Manifest;
