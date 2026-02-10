//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/** Adds the base path to the given path.
 * If the path starts with a slash, it will be removed before adding the base path.
 * */
export function basePath(path: string): string {
  const base = import.meta.env.BASE_URL;
  if (path.startsWith('/')) {
    path = path.substring(1);
  }
  if (base.endsWith('/')) {
    return `${base}${path}`;
  }

  return `${base}/${path}`;
}