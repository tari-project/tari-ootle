//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function clampPageIndex(page: number, rowsPerPage: number, totalItems: number): number {
  if (totalItems <= 0 || rowsPerPage <= 0 || page <= 0) {
    return 0;
  }
  const maxPage = Math.ceil(totalItems / rowsPerPage) - 1;
  return Math.min(page, maxPage);
}
