//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export const LEVELS = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"] as const;
export type Level = (typeof LEVELS)[number];

export interface Line {
  /** Byte offset of the line in the file: unique across chunks and stable when follow mode refetches the tail. */
  n: number;
  level: Level | null;
  text: string;
}

/** Matches log4rs (`YYYY-MM-DD HH:MM:SS...`) and RFC 3339 tracing timestamps, with optional `Z` or `±HH:MM` offset. */
const TIMESTAMP_PATTERN = "\\d{4}-\\d{2}-\\d{2}[T ]\\d+:\\d{2}:\\d{2}[.\\d]*(?:Z|[+-]\\d{2}:\\d{2})?";
export const TIME_RE = new RegExp(`^(${TIMESTAMP_PATTERN})`);
export const TARGET_RE = /\[([\w:]+::[\w:]+)\]/;
/**
 * log4rs writes the level after the timestamp and one or more bracketed fields (e.g. `[target]` or
 * `[target] [Thread:N]`); tracing lines are normalised into the same shape before level extraction.
 */
const HEADER_LEVEL_RE = new RegExp(
  `^(?:${TIMESTAMP_PATTERN})\\s+(?:\\[[^\\]]+\\]\\s+)*(ERROR|WARN|INFO|DEBUG|TRACE)\\b`,
);
/** Matches console/stdout lines without a full date prefix (e.g. `HH:MM INFO ...` or `[target] WARN ...`). */
const BARE_LEVEL_RE = /^\s*(?:\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?\s+)?(?:\[[^\]]+\]\s+)*(ERROR|WARN|INFO|DEBUG|TRACE)\b/;
/**
 * A tracing line is `<timestamp> <LEVEL> [<spans>: ]<target>: <message>`, with no brackets around the target.
 * Rewriting it into log4rs' shape lets one renderer highlight both.
 */
const TRACING_RE = /^(\d{4}-\d{2}-\d{2}T[\d:.]+(?:Z|[+-]\d{2}:\d{2}))\s+(ERROR|WARN|INFO|DEBUG|TRACE)\s+([\s\S]*)$/;
const TARGET_START_RE = /^([\w:]+::[\w:]+):(?:\s+([\s\S]*)|())$/;
const SPAN_NAME_RE = /^[\w-]+(?:::[\w-]+)*/;
const ANSI_RE = /\u001b\[[0-9;]*m/g;

const ENCODER = new TextEncoder();

/** Advances past a balanced `{...}` field list (respecting nested braces and quoted strings), or returns -1. */
function skipBalancedBraces(text: string, openIndex: number): number {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = openIndex; i < text.length; i++) {
    const ch = text[i];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (ch === "\\") {
        escaped = true;
      } else if (ch === '"') {
        inString = false;
      }
      continue;
    }
    if (ch === '"') {
      inString = true;
    } else if (ch === "{") {
      depth++;
    } else if (ch === "}") {
      depth--;
      if (depth === 0) {
        return i + 1;
      }
    }
  }
  return -1;
}

/**
 * Splits tracing's `[<span_1>:<span_2>: ]<module::target>: [<message>]` payload anchored at `rest` start.
 *
 * `tracing-subscriber`'s full format emits each span as `name{fields}:` (or `name:` when fieldless), joins nested
 * spans with no space, and writes a single space after the final span before the target. The target must directly
 * follow the span list, so a `::` token later in the message (`main: failed in std::io: ...`) cannot become it.
 */
function splitTracingTarget(rest: string): { spans: string; target: string; message: string } | null {
  const direct = TARGET_START_RE.exec(rest);
  if (direct) {
    return { spans: "", target: direct[1], message: direct[2] ?? "" };
  }

  let cursor = 0;
  for (;;) {
    const nameMatch = SPAN_NAME_RE.exec(rest.slice(cursor));
    if (!nameMatch) {
      return null;
    }
    cursor += nameMatch[0].length;
    if (rest[cursor] === "{") {
      cursor = skipBalancedBraces(rest, cursor);
      if (cursor === -1) {
        return null;
      }
    }
    if (rest[cursor] !== ":") {
      return null;
    }
    cursor++;

    // Nested tracing spans are joined by `:` with no space (`outer{a=1}:inner{b=2}: target::path: msg`).
    if (cursor < rest.length && !/\s/.test(rest[cursor])) {
      continue;
    }
    break;
  }

  const separator = /^\s+/.exec(rest.slice(cursor));
  if (!separator) {
    return null;
  }
  const targetMatch = TARGET_START_RE.exec(rest.slice(cursor + separator[0].length));
  if (!targetMatch) {
    return null;
  }
  return {
    spans: rest.slice(0, cursor - 1),
    target: targetMatch[1],
    message: targetMatch[2] ?? "",
  };
}

/**
 * Normalises one raw line to `<timestamp> [<target>] <LEVEL> <message>`.
 *
 * The indexer logs through `tracing`, which orders the fields differently from log4rs and, in files written before
 * ANSI output was turned off, wraps every one of them in colour escapes.
 */
export function normalise(raw: string): string {
  const withoutCr = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
  const text = withoutCr.replace(ANSI_RE, "");
  const tracing = TRACING_RE.exec(text);
  if (!tracing) {
    return text;
  }

  const [, time, level, rest] = tracing;
  const targeted = splitTracingTarget(rest);
  if (!targeted) {
    return `${time} ${level.padEnd(5)} ${rest}`;
  }

  const { spans, target, message } = targeted;
  const body = `${spans ? `${spans} ` : ""}${message}`;
  return body ? `${time} [${target}] ${level.padEnd(5)} ${body}` : `${time} [${target}] ${level.padEnd(5)}`;
}

/**
 * Groups lines into whole log entries.
 *
 * The view shows the newest entry first, and reversing bare lines to get there would turn every multi-line
 * entry - a record whose message contains newlines, a panic and its backtrace - upside down. Reversing entries
 * instead keeps each one readable top to bottom.
 *
 * A timestamped file starts a new entry at each timestamp. Raw stdout and stderr carry no timestamps, so each
 * line stands alone there, except inside a panic block: that runs from the `panicked at` header to the next
 * blank line, which is the one multi-line shape those files reliably contain.
 */
export function toEntries(lines: Line[]): Line[][] {
  const timestamped = lines.some((line) => TIME_RE.test(line.text));
  const entries: Line[][] = [];
  let inPanic = false;

  for (const line of lines) {
    const startsRecord = TIME_RE.test(line.text);
    if (startsRecord) {
      inPanic = false;
    } else if (!timestamped) {
      if (inPanic && line.text.trim() === "") {
        inPanic = false;
      } else if (!inPanic && line.text.includes("panicked at")) {
        inPanic = true;
        entries.push([line]);
        continue;
      }
    }

    const continues = !startsRecord && (timestamped ? entries.length > 0 : inPanic);
    if (continues) {
      entries[entries.length - 1].push(line);
    } else {
      entries.push([line]);
    }
  }

  return entries;
}

function extractLevel(line: string, timestamped: boolean): Level | null {
  if (TIME_RE.test(line)) {
    return (HEADER_LEVEL_RE.exec(line)?.[1] as Level) ?? null;
  }
  if (timestamped) {
    return null;
  }
  return (BARE_LEVEL_RE.exec(line)?.[1] as Level) ?? null;
}

/** Splits a chunk of file bytes into lines, keyed by their byte offset in the file. */
export function parse(body: string, start: number): Line[] {
  const raw = body.split("\n");
  // A chunk ends on a newline, so the split leaves a trailing empty element that is not a line of the file.
  if (raw.length && raw[raw.length - 1] === "") {
    raw.pop();
  }

  const normalisedLines = raw.map(normalise);
  const timestamped = normalisedLines.some((line) => TIME_RE.test(line));
  const lines: Line[] = [];
  let offset = start;
  for (let i = 0; i < raw.length; i++) {
    const normalised = normalisedLines[i];
    lines.push({ n: offset, level: extractLevel(normalised, timestamped), text: normalised });
    offset += ENCODER.encode(raw[i]).length + 1;
  }
  return lines;
}
