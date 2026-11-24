//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export const DEFAULT_TIME_CHIP_TOOLTIP_PREFIX = "Last updated";

export function formatTimeChipTooltip(
  formattedTime: string,
  prefix: string = DEFAULT_TIME_CHIP_TOOLTIP_PREFIX,
): string {
  const trimmedTime = formattedTime.trim();
  if (!trimmedTime) {
    return "";
  }
  const trimmedPrefix = prefix.trim();
  return trimmedPrefix ? `${trimmedPrefix}: ${trimmedTime}` : trimmedTime;
}
