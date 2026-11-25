//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { displayDuration } from "../src/utils/duration.ts";

describe("displayDuration", () => {
  it("truncates minutes instead of rounding them into the next minute", () => {
    assert.equal(displayDuration({ secs: 90, nanos: 0 }), "1m30s");
    assert.equal(displayDuration({ secs: 3599, nanos: 0 }), "59m59s");
    assert.equal(displayDuration({ secs: 60, nanos: 0 }), "1m0s");
    assert.equal(displayDuration({ secs: 59, nanos: 0 }), "59s");
  });

  it("truncates hours instead of rounding them into the next hour", () => {
    assert.equal(displayDuration({ secs: 5400, nanos: 0 }), "1h30m");
    assert.equal(displayDuration({ secs: 3661, nanos: 0 }), "1h1m");
    assert.equal(displayDuration({ secs: 3600, nanos: 0 }), "1h0m");
  });

  it("keeps sub-second formatting", () => {
    assert.equal(displayDuration({ secs: 0, nanos: 1_500_000 }), "1.50ms");
    assert.equal(displayDuration({ secs: 0, nanos: 2_500 }), "2.50µs");
    assert.equal(displayDuration({ secs: 0, nanos: 40 }), "40ns");
  });
});
