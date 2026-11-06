//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import Alert from "@mui/material/Alert";
import AlertTitle from "@mui/material/AlertTitle";
import Typography from "@mui/material/Typography";
import type { ComponentReference, ComponentUpgrade } from "@tari-project/ootle-ts-bindings";

function describeComponent(component: ComponentReference): string {
  if ("Address" in component) {
    return component.Address;
  }
  return `the component in workspace slot ${component.Workspace}, resolved when the transaction runs`;
}

/// Every `UpdateComponentTemplate` instruction in a transaction about to be signed. A signer who may upgrade the
/// component hands its code, and every vault it holds, to the new template.
export default function ComponentUpgradeWarning({ upgrades }: { upgrades: ComponentUpgrade[] | undefined }) {
  if (!upgrades || upgrades.length === 0) {
    return null;
  }
  return (
    <Alert severity="error" sx={{ mb: 2, mt: 2 }}>
      <AlertTitle>This transaction replaces component code</AlertTitle>
      <Typography variant="body2" sx={{ mb: 1 }}>
        If a key signing this transaction may upgrade a component listed here, the new template takes over the component
        and every vault it holds. Sign only if you intend this upgrade.
      </Typography>
      {upgrades.map((upgrade, i) => (
        <Typography key={i} variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
          {describeComponent(upgrade.component)} → template {upgrade.new_template}
          {upgrade.migrate ? `, then runs ${upgrade.migrate.name}` : ""}
        </Typography>
      ))}
    </Alert>
  );
}
