//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/**
 * Prepends the Astro base path to the given path, handling various edge cases.
 * @param {string} path - The path to prepend the base path to.
 * @returns {string} The path with the base path prepended.
 */
export function basePath(path: string): string {
  const base = import.meta.env.BASE_URL;
  // If base is not set, or is the root, return the path as-is.
  if (!base || base === "/") {
    return path;
  }
  // Remove trailing slash from base and leading slash from path to prevent double slashes.
  const strippedBase = base.endsWith("/") ? base.slice(0, -1) : base;
  const strippedPath = path.startsWith("/") ? path.substring(1) : path;
  return `${strippedBase}/${strippedPath}`;
}
