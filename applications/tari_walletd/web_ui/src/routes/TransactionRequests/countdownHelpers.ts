//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function getRemainingSeconds(expiresAt: bigint, nowSeconds: number): number {
  return Math.max(0, Math.floor(Number(expiresAt) - nowSeconds));
}

export function formatCountdownLabel(remainingSeconds: number): string {
  if (remainingSeconds <= 0) {
    return "expired";
  }
  const mins = Math.floor(remainingSeconds / 60);
  const secs = remainingSeconds % 60;
  return `expires in ${mins}:${secs.toString().padStart(2, "0")}`;
}
