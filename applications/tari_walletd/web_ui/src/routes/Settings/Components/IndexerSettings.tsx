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

import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import { GetNetworkInfoResponse } from "@tari-project/ootle-ts-bindings";
import { indexerGetNetworkInfo, settingsSet } from "@utils/json_rpc";
import React, { useEffect, useState } from "react";
import { Form } from "react-router-dom";

function validateUrl(value: string): string | null {
  try {
    const url = new URL(value);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return "URL must use http or https";
    }
    return null;
  } catch {
    return "Invalid URL";
  }
}

interface IndexerStatusProps {
  indexerUrl: string;
  walletNetwork: string;
}

function IndexerStatus({ indexerUrl, walletNetwork }: IndexerStatusProps) {
  const [loading, setLoading] = useState(false);
  const [info, setInfo] = useState<GetNetworkInfoResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!indexerUrl) return;
    setLoading(true);
    setInfo(null);
    setError(null);
    indexerGetNetworkInfo(indexerUrl)
      .then((res) => {
        setInfo(res);
        setError(null);
      })
      .catch((e) => {
        setError(e instanceof Error ? e.message : "Cannot connect to indexer");
        setInfo(null);
      })
      .finally(() => setLoading(false));
  }, [indexerUrl]);

  if (!indexerUrl) return null;

  if (loading) {
    return (
      <Box style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
        <CircularProgress size={14} />
        <Typography variant="caption" color="text.secondary">
          Checking connection…
        </Typography>
      </Box>
    );
  }

  if (error) {
    return (
      <Alert severity="error" style={{ marginTop: 8 }}>
        Cannot connect to indexer: {error}
      </Alert>
    );
  }

  if (info) {
    const networkMismatch = walletNetwork && info.network !== walletNetwork;
    return (
      <Box style={{ marginTop: 8, display: "flex", flexDirection: "column", gap: 4 }}>
        {networkMismatch && (
          <Alert severity="warning">
            Network mismatch: wallet is on <strong>{walletNetwork}</strong> but indexer is on{" "}
            <strong>{info.network}</strong>
          </Alert>
        )}
        <Typography variant="caption" color="text.secondary">
          Network: <strong>{info.network}</strong> &nbsp;·&nbsp; Epoch: <strong>{info.epoch}</strong>
        </Typography>
      </Box>
    );
  }

  return null;
}

interface IndexerSettingsProps {
  indexerUrl: string;
  walletNetwork: string;
}

function IndexerSettings({ indexerUrl, walletNetwork }: IndexerSettingsProps) {
  const [inputUrl, setInputUrl] = useState(indexerUrl);
  const [showForm, setShowForm] = useState(false);
  const [currentUrl, setCurrentUrl] = useState(indexerUrl);
  const [validationError, setValidationError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    setCurrentUrl(indexerUrl);
    setInputUrl(indexerUrl);
  }, [indexerUrl]);

  const onInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setInputUrl(e.target.value);
    setValidationError(validateUrl(e.target.value));
    setSaveError(null);
  };

  const onSubmitIndexer = async () => {
    const error = validateUrl(inputUrl);
    if (error) {
      setValidationError(error);
      return;
    }
    try {
      await settingsSet({ indexer_url: inputUrl, advanced_ui_features: null, claimed_accounts: null });
      setCurrentUrl(inputUrl);
      setShowForm(false);
      setSaveError(null);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : "Failed to save indexer URL");
    }
  };

  const onCancel = () => {
    setShowForm(false);
    setValidationError(null);
    setSaveError(null);
  };

  return (
    <>
      <Box className="flex-container">
        {showForm ? (
          <Form
            onSubmit={onSubmitIndexer}
            className="flex-container"
            style={{ alignItems: "flex-start", flexDirection: "column", width: "100%" }}
          >
            <Box className="flex-container" style={{ alignItems: "center", width: "100%" }}>
              <TextField
                name="indexer_url"
                label="Indexer url"
                value={inputUrl}
                onChange={onInputChange}
                size="small"
                style={{ flexGrow: 1 }}
                error={!!validationError}
                helperText={validationError ?? " "}
              />
              <Button variant="contained" type="submit" disabled={!!validationError}>
                Set Indexer
              </Button>
              <Button variant="outlined" onClick={onCancel}>
                Cancel
              </Button>
            </Box>
            {saveError && (
              <Alert severity="error" style={{ marginTop: 8, width: "100%" }}>
                {saveError}
              </Alert>
            )}
          </Form>
        ) : (
          <Box style={{ width: "100%" }}>
            <Box className="flex-container" style={{ justifyContent: "space-between", alignItems: "center" }}>
              {currentUrl === "" ? (
                <Alert severity="warning" style={{ width: "100%" }}>
                  No Indexer Set
                </Alert>
              ) : (
                <Typography variant="body2">{currentUrl}</Typography>
              )}
              <Button
                variant="outlined"
                onClick={() => {
                  setInputUrl(currentUrl);
                  setValidationError(null);
                  setSaveError(null);
                  setShowForm(true);
                }}
              >
                Set new url
              </Button>
            </Box>
            <IndexerStatus indexerUrl={currentUrl} walletNetwork={walletNetwork} />
          </Box>
        )}
      </Box>
    </>
  );
}

export default IndexerSettings;
