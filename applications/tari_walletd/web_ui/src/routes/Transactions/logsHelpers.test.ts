//  Copyright 2026. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import { describe, expect, it } from "vitest";
import { getLogLevelColor, normalizeLogLevelLabel } from "./logsHelpers";

describe("logsHelpers", () => {
  it("maps standard log levels case-insensitively and ignores surrounding whitespace", () => {
    expect(getLogLevelColor("ERROR")).toBe("error");
    expect(getLogLevelColor(" warn ")).toBe("warning");
    expect(getLogLevelColor("Warning")).toBe("warning");
    expect(getLogLevelColor("Info")).toBe("info");
    expect(getLogLevelColor("DEBUG")).toBe("secondary");
    expect(getLogLevelColor("trace")).toBe("default");
  });

  it("handles missing or unrecognized log levels safely", () => {
    expect(getLogLevelColor(undefined)).toBe("primary");
    expect(getLogLevelColor(null)).toBe("primary");
    expect(getLogLevelColor("")).toBe("primary");
    expect(getLogLevelColor("custom")).toBe("primary");

    expect(normalizeLogLevelLabel(undefined)).toBe("Unknown");
    expect(normalizeLogLevelLabel("   ")).toBe("Unknown");
    expect(normalizeLogLevelLabel(" Info ")).toBe("Info");
  });
});
