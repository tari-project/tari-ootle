// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

const TEMPLATE_PREFIX = "template_";

export function normalizeTemplateAddress(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) {
    return "";
  }
  return trimmed.startsWith(TEMPLATE_PREFIX) ? trimmed.slice(TEMPLATE_PREFIX.length).trim() : trimmed;
}

export type TemplateLookupAction = { kind: "ignore" } | { kind: "set-address"; address: string } | { kind: "refetch" };

export function resolveTemplateLookupAction(input: string, activeAddress: string): TemplateLookupAction {
  const normalized = normalizeTemplateAddress(input);
  if (!normalized) {
    return { kind: "ignore" };
  }
  if (normalized === activeAddress) {
    return { kind: "refetch" };
  }
  return { kind: "set-address", address: normalized };
}
