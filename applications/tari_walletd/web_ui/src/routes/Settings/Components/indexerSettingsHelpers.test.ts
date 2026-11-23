//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { normalizeIndexerUrl, validateIndexerUrl } from "./indexerSettingsHelpers";

describe("indexerSettingsHelpers", () => {
  it("trims surrounding whitespace in normalizeIndexerUrl", () => {
    expect(normalizeIndexerUrl("  https://indexer.tari.com/  ")).toBe("https://indexer.tari.com/");
  });

  it("accepts valid http and https URLs", () => {
    expect(validateIndexerUrl("http://127.0.0.1:18300")).toBeNull();
    expect(validateIndexerUrl(" https://indexer.tari.com ")).toBeNull();
  });

  it("rejects non-http(s) protocols and invalid URLs", () => {
    expect(validateIndexerUrl("ws://127.0.0.1:18300")).toBe("URL must use http or https");
    expect(validateIndexerUrl("ftp://example.com")).toBe("URL must use http or https");
    expect(validateIndexerUrl("not-a-url")).toBe("Invalid URL");
    expect(validateIndexerUrl("   ")).toBe("Invalid URL");
  });

  it("detects duplicates even when the input has surrounding whitespace", () => {
    const existing = ["https://indexer.tari.com"];
    expect(validateIndexerUrl("https://indexer.tari.com", existing)).toBe("This indexer is already configured");
    expect(validateIndexerUrl("  https://indexer.tari.com  ", existing)).toBe("This indexer is already configured");
  });
});
