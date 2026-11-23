//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function validateIndexerUrl(value: string, existingUrls: readonly string[] = []): string | null {
  let url: URL;
  try {
    url = new URL(value);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return "URL must use http or https";
    }
  } catch {
    return "Invalid URL";
  }
  if (existingUrls.includes(url.href)) {
    return "This indexer is already configured";
  }
  return null;
}
