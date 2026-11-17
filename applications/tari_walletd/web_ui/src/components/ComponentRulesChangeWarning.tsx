//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import Alert from "@mui/material/Alert";
import AlertTitle from "@mui/material/AlertTitle";
import Typography from "@mui/material/Typography";
import type { AccessRule, ComponentReference, ComponentRulesChange, OwnerRule } from "@tari-project/ootle-ts-bindings";

function describeComponent(component: ComponentReference): string {
  if ("Address" in component) {
    return component.Address;
  }
  return `the component in workspace slot ${component.Workspace}, resolved when the transaction runs`;
}

function describeAccessRule(rule: AccessRule): string {
  if (rule === "AllowAll") {
    return "anyone";
  }
  if (rule === "DenyAll") {
    return "nobody";
  }
  return JSON.stringify(rule.Restricted);
}

function describeOwnerRule(rule: OwnerRule): string {
  if (rule === "OwnedBySigner") {
    return "the transaction's main signer";
  }
  if (rule === "None") {
    return "no owner";
  }
  if ("ByPublicKey" in rule) {
    return `public key ${rule.ByPublicKey.map((b) => b.toString(16).padStart(2, "0")).join("")}`;
  }
  return `whoever satisfies ${describeAccessRule(rule.ByAccessRule)}`;
}

function describeChange(change: ComponentRulesChange): string {
  if ("OwnerRule" in change) {
    const { component, owner_rule } = change.OwnerRule;
    return `${describeComponent(component)}: owner → ${describeOwnerRule(owner_rule)}`;
  }
  const { component, access_rules } = change.AccessRules;
  const methods = Object.entries(access_rules.method_access)
    .map(([method, rule]) => `${method}: ${describeAccessRule(rule)}`)
    .join(", ");
  const rules = methods
    ? `${methods}; other methods: ${describeAccessRule(access_rules.default)}`
    : `every method: ${describeAccessRule(access_rules.default)}`;
  return `${describeComponent(component)}: access → ${rules}`;
}

/// Every `SetComponentAccessRules` and `SetComponentOwnerRule` instruction in a transaction about to be signed. A
/// signer who satisfies the component's rules updater decides who may call its methods, or who owns it and every
/// vault it holds.
export default function ComponentRulesChangeWarning({ changes }: { changes: ComponentRulesChange[] | undefined }) {
  if (!changes || changes.length === 0) {
    return null;
  }
  return (
    <Alert severity="error" sx={{ mb: 2, mt: 2 }}>
      <AlertTitle>This transaction changes who controls a component</AlertTitle>
      <Typography variant="body2" sx={{ mb: 1 }}>
        If a key signing this transaction may change the rules of a component listed here, the new owner or access rules
        take effect on submission. A new owner controls the component and every vault it holds. Sign only if you intend
        this change.
      </Typography>
      {changes.map((change, i) => (
        <Typography key={i} variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
          {describeChange(change)}
        </Typography>
      ))}
    </Alert>
  );
}
