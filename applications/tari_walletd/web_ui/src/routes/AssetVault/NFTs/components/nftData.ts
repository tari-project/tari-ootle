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

export function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isPrimitive(value: unknown): value is string | number | boolean | bigint {
  return (
    typeof value === "string" || typeof value === "number" || typeof value === "boolean" || typeof value === "bigint"
  );
}

export function getNftImageUrl(mutableData: unknown): string | undefined {
  if (!isPlainRecord(mutableData)) {
    return undefined;
  }
  const url = mutableData.image_url;
  return typeof url === "string" && url.trim().length > 0 ? url.trim() : undefined;
}

export function formatNftDataValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "";
  }
  if (isPrimitive(value)) {
    return value.toString();
  }
  return JSON.stringify(value, (_key, nested) => (typeof nested === "bigint" ? nested.toString() : nested));
}

export function extractNftMetadata(data: unknown, limit = 5): [string, string][] {
  const metadata: [string, string][] = [];
  if (!isPlainRecord(data)) {
    return metadata;
  }
  for (const [key, value] of Object.entries(data)) {
    if (isPrimitive(value)) {
      metadata.push([key, value.toString()]);
      if (metadata.length >= limit) break;
    }
  }
  return metadata;
}
