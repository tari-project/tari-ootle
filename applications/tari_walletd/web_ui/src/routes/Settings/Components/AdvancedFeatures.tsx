// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Checkbox from "@mui/material/Checkbox";
import FormControlLabel from "@mui/material/FormControlLabel";
import useSettingsStore from "@store/settingsStore";
import { settingsGet, settingsSet } from "@utils/json_rpc";
import { useEffect } from "react";

function AdvancedFeatures() {
  const { advancedUiFeatures, setAdvancedUiFeatures } = useSettingsStore();

  useEffect(() => {
    settingsGet().then((res) => {
      setAdvancedUiFeatures(res.advanced_ui_features);
    });
  }, [setAdvancedUiFeatures]);

  const onManifestChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const updated = { ...advancedUiFeatures, enable_manifest: e.target.checked };
    setAdvancedUiFeatures(updated);
    const current = await settingsGet();
    settingsSet({ indexer_url: current.indexer_url, advanced_ui_features: updated, claimed_accounts: null });
  };

  return (
    <Box>
      <Alert severity="warning" style={{ marginBottom: 16 }}>
        These features are experimental. No support or documentation is provided. Use at your own risk.
      </Alert>
      <FormControlLabel
        control={<Checkbox checked={advancedUiFeatures.enable_manifest} onChange={onManifestChange} />}
        label="Enable manifest transaction builder"
      />
    </Box>
  );
}

export default AdvancedFeatures;
