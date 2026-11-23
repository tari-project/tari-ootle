//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export type ParsedManifestFee = { kind: "estimate" } | { kind: "submit"; value: bigint } | { kind: "invalid" };

/**
 * Indent manifest code by tracking opening and closing braces on each line.
 */
export function formatManifestCode(code: string): string {
  const lines = code.split("\n");
  const formatted: string[] = [];
  let indent = 0;

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();
    if (!trimmed) {
      formatted.push("");
      continue;
    }

    const leadingCloses = trimmed.match(/^}+/)?.[0].length ?? 0;
    if (leadingCloses > 0) {
      indent = Math.max(0, indent - leadingCloses);
    }

    formatted.push("    ".repeat(indent) + trimmed);

    const opens = (trimmed.match(/{/g) || []).length;
    const closes = (trimmed.match(/}/g) || []).length;
    indent = Math.max(0, indent + opens - (closes - leadingCloses));
  }

  return formatted.join("\n");
}

const MAX_U64 = 18446744073709551615n;

/**
 * Parse the Manifest editor Max Fee field without throwing.
 * Empty input or zero means dry-run fee estimation; a positive base-10 u64 integer submits with that max_fee.
 */
export function parseManifestFee(raw: string): ParsedManifestFee {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { kind: "estimate" };
  }
  if (!/^\d+$/.test(trimmed)) {
    return { kind: "invalid" };
  }
  const value = BigInt(trimmed);
  if (value > MAX_U64) {
    return { kind: "invalid" };
  }
  if (value === 0n) {
    return { kind: "estimate" };
  }
  return { kind: "submit", value };
}
