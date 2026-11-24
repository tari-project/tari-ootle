// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Checkbox from "@mui/material/Checkbox";
import FormControlLabel from "@mui/material/FormControlLabel";
import useSettingsStore from "@store/settingsStore";
import { AdvancedUiFeatures } from "@tari-project/ootle-ts-bindings";
import { settingsGet, settingsSet } from "@utils/json_rpc";
import { useEffect, useRef, useState } from "react";
import { formatSettingsError, persistAdvancedFeatureToggle } from "./advancedFeaturesHelpers";

function AdvancedFeatures() {
  const { advancedUiFeatures, setAdvancedUiFeatures } = useSettingsStore();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const userToggledRef = useRef(false);
  const mountedRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    mountedRef.current = true;
    settingsGet()
      .then((res) => {
        if (!cancelled && !userToggledRef.current) {
          setAdvancedUiFeatures(res.advanced_ui_features);
          setError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(formatSettingsError(err, "Failed to load advanced feature settings."));
        }
      });
    return () => {
      cancelled = true;
      mountedRef.current = false;
    };
  }, [setAdvancedUiFeatures]);

  const setFeature = (feature: keyof AdvancedUiFeatures) => async (e: React.ChangeEvent<HTMLInputElement>) => {
    if (saving) {
      return;
    }
    userToggledRef.current = true;
    setSaving(true);
    setError(null);
    const result = await persistAdvancedFeatureToggle({
      current: advancedUiFeatures,
      feature,
      enabled: e.target.checked,
      apply: setAdvancedUiFeatures,
      persist: (updated) =>
        settingsSet({ indexer_url: null, indexer_urls: null, advanced_ui_features: updated, claimed_accounts: null }),
    });
    if (mountedRef.current) {
      if (!result.ok) {
        setError(result.error);
      }
      setSaving(false);
    }
  };

  return (
    <Box>
      <Alert severity="warning" style={{ marginBottom: 16 }}>
        These features are experimental. No support or documentation is provided. Use at your own risk.
      </Alert>
      {error && (
        <Alert severity="error" style={{ marginBottom: 16 }}>
          {error}
        </Alert>
      )}
      <FormControlLabel
        control={
          <Checkbox
            checked={advancedUiFeatures.enable_manifest}
            onChange={setFeature("enable_manifest")}
            disabled={saving}
          />
        }
        label="Enable manifest transaction builder"
      />
      <FormControlLabel
        control={
          <Checkbox
            checked={advancedUiFeatures.enable_validator_set}
            onChange={setFeature("enable_validator_set")}
            disabled={saving}
          />
        }
        label="Show the validator set page"
      />
    </Box>
  );
}

export default AdvancedFeatures;
