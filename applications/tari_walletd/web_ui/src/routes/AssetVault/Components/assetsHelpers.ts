//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function clampNftPage(page: number, totalCount: number, rowsPerPage: number): number {
  if (totalCount <= 0 || rowsPerPage <= 0 || page <= 0) {
    return 0;
  }
  const maxPage = Math.ceil(totalCount / rowsPerPage) - 1;
  return Math.min(page, maxPage);
}
