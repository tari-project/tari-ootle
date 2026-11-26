//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function getValidatedRename(draftName: string, currentName?: string | null): string | null {
  const trimmed = draftName.trim();
  if (!trimmed || trimmed === (currentName ?? "")) {
    return null;
  }
  return trimmed;
}

export function runIfAccountActive(targetAccount: string, activeAccount: string | null, callback: () => void): boolean {
  if (activeAccount !== targetAccount) {
    return false;
  }
  callback();
  return true;
}
