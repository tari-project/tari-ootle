//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { normalise, parse } from "../src/routes/logFormat.ts";

const T = "2026-01-01T00:00:00.123456Z";

describe("normalise tracing lines", () => {
  it("brackets a target with no spans", () => {
    assert.equal(normalise(`${T}  INFO tari_indexer::x: hello`), `${T} [tari_indexer::x] INFO  hello`);
  });

  it("keeps a single fieldless span ahead of the target", () => {
    assert.equal(
      normalise(`${T}  INFO sync: tari_indexer::x: src/x.rs:12: msg`),
      `${T} [tari_indexer::x] INFO  sync src/x.rs:12: msg`,
    );
  });

  it("keeps a single span with fields ahead of the target", () => {
    assert.equal(
      normalise(`${T}  WARN sync{epoch=3}: tari_indexer::x: msg`),
      `${T} [tari_indexer::x] WARN  sync{epoch=3} msg`,
    );
  });

  it("keeps adjacent nested spans", () => {
    assert.equal(
      normalise(`${T} DEBUG outer{a=1}:inner:last{b=2}: target::path: msg`),
      `${T} [target::path] DEBUG outer{a=1}:inner:last{b=2} msg`,
    );
  });

  it("accepts span names containing `::` and fields with nested braces or quotes", () => {
    assert.equal(
      normalise(`${T}  INFO rpc::internal::timing{peer=Foo { id: 1 } s="}"}: tari::rpc: done`),
      `${T} [tari::rpc] INFO  rpc::internal::timing{peer=Foo { id: 1 } s="}"} done`,
    );
  });

  it("does not take a `::` token from the message of a single-segment target", () => {
    assert.equal(
      normalise(`${T} ERROR main: failed in std::io: broken pipe`),
      `${T} ERROR main: failed in std::io: broken pipe`,
    );
  });

  it("does not read a target after spans separated by spaces", () => {
    assert.equal(
      normalise(`${T}  INFO span: other{a=1}: target::x: msg`),
      `${T} INFO  span: other{a=1}: target::x: msg`,
    );
  });

  it("accepts RFC 3339 offsets and empty messages", () => {
    assert.equal(normalise("2026-01-01T00:00:00.1+02:00  INFO a::b:"), "2026-01-01T00:00:00.1+02:00 [a::b] INFO ");
  });
});

describe("parse", () => {
  it("strips CRLF without shifting byte offsets", () => {
    const lines = parse(`${T}  INFO a::b: é\r\n${T}  WARN a::b: x\r\n`, 10);
    assert.deepEqual(
      lines.map(({ n, level, text }) => [n, level, text]),
      [
        [10, "INFO", `${T} [a::b] INFO  é`],
        [10 + T.length + 16 + 1, "WARN", `${T} [a::b] WARN  x`],
      ],
    );
  });

  it("reads the level after multiple bracketed log4rs fields", () => {
    const [line] = parse("2026-01-01 00:00:00.123 [wallet::x] [Thread:1234] INFO  msg\n", 0);
    assert.equal(line.level, "INFO");
  });

  it("gives no level to continuation lines in a timestamped chunk", () => {
    const lines = parse(`${T}  INFO a::b: first\nERROR looks like a level\n`, 0);
    assert.deepEqual(lines.map((line) => line.level), ["INFO", null]);
  });

  it("reads bare levels when the chunk has no timestamps", () => {
    const lines = parse("12:00 WARN careful\n[target] ERROR bad\n", 0);
    assert.deepEqual(lines.map((line) => line.level), ["WARN", "ERROR"]);
  });
});
