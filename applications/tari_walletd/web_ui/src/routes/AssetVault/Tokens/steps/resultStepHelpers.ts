//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export const AUTO_CLOSE_DELAY_MS = 10_000;

export function shouldAutoCloseTransferResult(disabled: boolean, transferResult?: { success: boolean }): boolean {
  return !disabled && transferResult?.success === true;
}
