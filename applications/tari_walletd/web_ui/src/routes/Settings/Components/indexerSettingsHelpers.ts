//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function normalizeIndexerUrl(value: string): string {
  return value.trim();
}

export function validateIndexerUrl(value: string, existingUrls: readonly string[] = []): string | null {
  const trimmed = normalizeIndexerUrl(value);
  try {
    const url = new URL(trimmed);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return "URL must use http or https";
    }
  } catch {
    return "Invalid URL";
  }
  if (existingUrls.includes(trimmed)) {
    return "This indexer is already configured";
  }
  return null;
}
