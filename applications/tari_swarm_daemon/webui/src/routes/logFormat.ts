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

/** log4rs writes the level bare; tracing writes it after an RFC 3339 timestamp. Both are matched here. */
const TIMESTAMP_PATTERN = "\\d{4}-\\d{2}-\\d{2}[T ]\\d+:\\d{2}:\\d{2}[.\\d]*(?:Z|[+-]\\d{2}:\\d{2})?";
export const TIME_RE = new RegExp(`^(${TIMESTAMP_PATTERN})`);
export const TARGET_RE = /\[([\w:]+::[\w:]+)\]/;
/** Matches the log level in a timestamped header (before the message body) to avoid false matches in message text. */
const HEADER_LEVEL_RE = new RegExp(
  `^(?:${TIMESTAMP_PATTERN})\\s+(?:\\[[^\\]]+\\]\\s+)?(ERROR|WARN|INFO|DEBUG|TRACE)\\b`,
);
const BARE_LEVEL_RE =
  /^\s*(?:\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?\s+)?(?:\[[^\]]+\]\s+)?(ERROR|WARN|INFO|DEBUG|TRACE)\b/;
/**
 * A tracing line is `<timestamp> <LEVEL> [<spans>: ]<target>: <message>`, with no brackets around the target.
 * Rewriting it into log4rs' shape lets one renderer highlight both.
 */
const TRACING_RE = /^(\d{4}-\d{2}-\d{2}T[\d:.]+(?:Z|[+-]\d{2}:\d{2}))\s+(ERROR|WARN|INFO|DEBUG|TRACE)\s+([\s\S]*)$/;
/** Matches optional tracing span prefixes (`name{...}: `) followed by a `module::path` target at `rest` start. */
const TRACING_SPANS_AND_TARGET_RE = /^((?:[\w-]+(?:\{[^}]*\})?:\s+)*)([\w:]+::[\w:]+):\s*([\s\S]*)$/;
const ANSI_RE = /\u001b\[[0-9;]*m/g;

const ENCODER = new TextEncoder();

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
  const targeted = TRACING_SPANS_AND_TARGET_RE.exec(rest);
  if (!targeted) {
    return `${time} ${level.padEnd(5)} ${rest}`;
  }

  const [, rawSpans, target, message] = targeted;
  const spans = rawSpans.replace(/:\s*$/, "").trim();
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

function extractLevel(line: string): Level | null {
  const match = TIME_RE.test(line) ? HEADER_LEVEL_RE.exec(line) : BARE_LEVEL_RE.exec(line);
  return (match?.[1] as Level) ?? null;
}

/** Splits a chunk of file bytes into lines, keyed by their byte offset in the file. */
export function parse(body: string, start: number): Line[] {
  const raw = body.split("\n");
  // A chunk ends on a newline, so the split leaves a trailing empty element that is not a line of the file.
  if (raw.length && raw[raw.length - 1] === "") {
    raw.pop();
  }

  const lines: Line[] = [];
  let offset = start;
  for (const text of raw) {
    const normalised = normalise(text);
    lines.push({ n: offset, level: extractLevel(normalised), text: normalised });
    offset += ENCODER.encode(text).length + 1;
  }
  return lines;
}
