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

import { Alert, Button, Chip, Divider } from "@mui/material";
import Box from "@mui/material/Box";
import { useTheme } from "@mui/material/styles";
import Typography from "@mui/material/Typography";
import { SettingsGetResponse } from "@tari-project/ootle-ts-bindings";
import { settingsGet } from "@utils/json_rpc";
import React, { useCallback, useEffect, useRef, useState } from "react";
import { formatSettingsError, getNetworkColor, shouldApplySettingsResult } from "./generalSettingsHelpers";
import IndexerSettings from "./IndexerSettings";

function GeneralSettings() {
  const theme = useTheme();
  const [settings, setSettings] = useState<SettingsGetResponse | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const mountedRef = useRef(false);
  const requestIdRef = useRef(0);

  const loadSettings = useCallback(() => {
    const requestId = ++requestIdRef.current;
    setLoadError(null);
    settingsGet()
      .then((res) => {
        if (shouldApplySettingsResult(requestId, requestIdRef.current, mountedRef.current)) {
          setSettings(res);
        }
      })
      .catch((err) => {
        if (shouldApplySettingsResult(requestId, requestIdRef.current, mountedRef.current)) {
          setLoadError(formatSettingsError(err));
        }
      });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    loadSettings();
    return () => {
      mountedRef.current = false;
    };
  }, [loadSettings]);

  const items = [
    {
      label: "Network",
      content: <NetworkSettings network={settings?.network.name ?? ""} />,
    },
    {
      label: "Indexers",
      content: (
        <IndexerSettings
          indexerUrls={settings?.indexer_urls ?? []}
          activeUrl={settings?.indexer_url ?? ""}
          walletNetwork={settings?.network.name ?? ""}
          onSaved={loadSettings}
        />
      ),
    },
  ];

  const renderedItems = items.map((item) => {
    return (
      <React.Fragment key={item.label}>
        <Typography>{item.label}</Typography>
        <Box>{item.content}</Box>
        <Divider />
      </React.Fragment>
    );
  });

  return (
    <Box
      style={{
        display: "flex",
        flexDirection: "column",
        gap: theme.spacing(3),
        paddingTop: theme.spacing(3),
      }}
    >
      {loadError && (
        <Alert
          severity="error"
          action={
            <Button color="inherit" size="small" onClick={loadSettings}>
              Retry
            </Button>
          }
        >
          {loadError}
        </Alert>
      )}
      {renderedItems}
    </Box>
  );
}

function NetworkSettings({ network }: { network: string }) {
  const color = getNetworkColor(network);

  return (
    <Chip
      label={network || "…"}
      size="small"
      style={{ backgroundColor: color, color: "#fff", fontWeight: 600, textTransform: "capitalize" }}
    />
  );
}

export default GeneralSettings;
