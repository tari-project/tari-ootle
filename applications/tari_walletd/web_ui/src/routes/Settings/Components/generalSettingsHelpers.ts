// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

const DEFAULT_NETWORK_COLOR = "#757575";

export const NETWORK_COLORS: Record<string, string> = {
  mainnet: "#4caf50",
  stagenet: "#9c27b0",
  nextnet: "#2196f3",
  localnet: "#ff9800",
  igor: "#00bcd4",
  esmeralda: "#009688",
};

export function getNetworkColor(network: string): string {
  return NETWORK_COLORS[network.trim().toLowerCase()] ?? DEFAULT_NETWORK_COLOR;
}

export function shouldApplySettingsResult(requestId: number, latestRequestId: number, isMounted: boolean): boolean {
  return isMounted && requestId === latestRequestId;
}

export function formatSettingsError(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }
  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }
  return "Failed to load settings.";
}
