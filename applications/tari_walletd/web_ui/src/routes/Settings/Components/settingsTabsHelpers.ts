// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export interface LabeledTabItem {
  label: string;
}

/**
 * Resolves the active MUI Tabs index from the selected tab label.
 * Keeps the user on the same logical tab if items are inserted, removed, or
 * reordered, falls back to index 0 when the selected label is no longer
 * present, and returns `false` when the menu is empty.
 */
export function resolveActiveTabIndex(
  menuItems: readonly LabeledTabItem[],
  selectedLabel: string | null,
): number | false {
  if (menuItems.length === 0) {
    return false;
  }
  if (selectedLabel !== null) {
    const matchIndex = menuItems.findIndex((item) => item.label === selectedLabel);
    if (matchIndex !== -1) {
      return matchIndex;
    }
  }
  return 0;
}
