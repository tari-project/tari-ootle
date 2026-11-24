//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function getValidatedRename(draftName: string, currentName?: string | null): string | null {
  const trimmed = draftName.trim();
  if (!trimmed || trimmed === (currentName ?? "")) {
    return null;
  }
  return trimmed;
}

export function applyRenameSuccessIfActive(
  targetAccount: string,
  activeAccount: string | null,
  trimmedName: string,
  callbacks: {
    closeEdit: () => void;
    onRenameSuccess?: (newName: string) => void;
  },
): boolean {
  if (activeAccount !== targetAccount) {
    return false;
  }
  callbacks.closeEdit();
  callbacks.onRenameSuccess?.(trimmedName);
  return true;
}
