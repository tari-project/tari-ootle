//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { toHexString } from "@utils/helpers";
import { describe, expect, it } from "vitest";
import { decodeDestinationTagAndBytes, tryDecodeUtf8 } from "./memoHelpers";

describe("memoHelpers", () => {
  describe("tryDecodeUtf8", () => {
    it("decodes valid UTF-8 byte sequences", () => {
      const bytes = new TextEncoder().encode("hello tari");
      expect(tryDecodeUtf8(bytes)).toBe("hello tari");
    });

    it("returns null for invalid UTF-8 sequences instead of replacement characters", () => {
      const invalidUtf8 = new Uint8Array([0xff, 0xfe, 0x80]);
      expect(tryDecodeUtf8(invalidUtf8)).toBeNull();
    });
  });

  describe("decodeDestinationTagAndBytes", () => {
    it("decodes UTF-8 destination tag and message", () => {
      const tag = new TextEncoder().encode("acct-42");
      const msg = new TextEncoder().encode("invoice #9");
      const payload = toHexString(new Uint8Array([tag.length, ...tag, ...msg]));
      expect(decodeDestinationTagAndBytes(payload)).toEqual({
        destinationTag: "acct-42",
        message: "invoice #9",
      });
    });

    it("falls back to hex for non-UTF-8 destination tag and message bytes", () => {
      const payload = toHexString(new Uint8Array([2, 0xff, 0xfe, 0x80, 0x81]));
      expect(decodeDestinationTagAndBytes(payload)).toEqual({
        destinationTag: "fffe",
        message: "8081",
      });
    });

    it("handles empty tag or empty message cleanly", () => {
      const msg = new TextEncoder().encode("only message");
      expect(decodeDestinationTagAndBytes(toHexString(new Uint8Array([0, ...msg])))).toEqual({
        destinationTag: "",
        message: "only message",
      });

      const tag = new TextEncoder().encode("tag1");
      expect(decodeDestinationTagAndBytes(toHexString(new Uint8Array([tag.length, ...tag])))).toEqual({
        destinationTag: "tag1",
        message: "",
      });
    });

    it("returns null for empty, malformed, or truncated payloads", () => {
      expect(decodeDestinationTagAndBytes("")).toBeNull();
      expect(decodeDestinationTagAndBytes("abc")).toBeNull();
      expect(decodeDestinationTagAndBytes("zzzz")).toBeNull();
      // Declares 5-byte tag but only provides 2 bytes after length prefix
      expect(decodeDestinationTagAndBytes("05aabb")).toBeNull();
    });
  });
});
