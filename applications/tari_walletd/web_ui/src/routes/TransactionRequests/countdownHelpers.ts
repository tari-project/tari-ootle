//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function getRemainingSeconds(expiresAt: bigint, nowSeconds: number): number {
  return Math.max(0, Math.ceil(Number(expiresAt) - nowSeconds));
}

export function formatCountdownLabel(remainingSeconds: number): string {
  if (remainingSeconds <= 0) {
    return "expired";
  }
  const hours = Math.floor(remainingSeconds / 3600);
  const mins = Math.floor((remainingSeconds % 3600) / 60);
  const secs = remainingSeconds % 60;
  const duration = hours > 0 ? `${hours}h ${mins}m` : `${mins}:${secs.toString().padStart(2, "0")}`;
  return `expires in ${duration}`;
}
