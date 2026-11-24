// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export function formatRelativeTime(date: Date, nowMs: number = Date.now()): string {
  const diffSeconds = Math.floor((nowMs - date.getTime()) / 1000);
  if (!Number.isFinite(diffSeconds)) {
    return "Unknown";
  }
  const seconds = Math.max(0, diffSeconds);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export function formatUnixTime(seconds: number | null | undefined, nowMs: number = Date.now()): string {
  if (seconds == null) {
    return "Never";
  }
  if (!Number.isFinite(seconds) || seconds < 0) {
    return "Invalid timestamp";
  }
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) {
    return "Invalid timestamp";
  }
  return `${date.toLocaleString()} (${formatRelativeTime(date, nowMs)})`;
}

export function formatNetworkEpoch(networkEpoch: number | null | undefined, isLoading: boolean): string {
  if (networkEpoch != null) {
    return String(networkEpoch);
  }
  if (isLoading) {
    return "Loading...";
  }
  return "Unknown (indexer unreachable)";
}
