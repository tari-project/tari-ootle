//  Copyright 2022. The Tari Project
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

import { ChangeEvent } from "react";
import type { FinalizeResult, SubstateId, Transaction, TransactionStatus } from "@tari-project/typescript-bindings";

export const renderJson = (json: any) => {
  if (Array.isArray(json)) {
    if (json.length == 32) {
      return <span className="string">"{toHexString(json)}"</span>;
    }
    return (
      <>
        [
        <ol>
          {json.map((val, index) => (
            <li key={index}>{renderJson(val)},</li>
          ))}
        </ol>
        ],
      </>
    );
  } else if (typeof json === "object") {
    return (
      <>
        {"{"}
        <ul>
          {Object.keys(json).map((key, index) => (
            <li key={index}>
              <b>"{key}"</b>:{renderJson(json[key])}
            </li>
          ))}
        </ul>
        {"}"}
      </>
    );
  } else {
    if (typeof json === "string") return <span className="string">"{json}"</span>;
    return <span className="other">{json}</span>;
  }
};

export function toHexString(byteArray: any): string {
  if (Array.isArray(byteArray)) {
    return Array.from(byteArray, function (byte) {
      return ("0" + (byte & 0xff).toString(16)).slice(-2);
    }).join("");
  }
  if (byteArray === undefined) {
    return "undefined";
  }
  // object might be a tagged object
  if (byteArray["@@TAGGED@@"] !== undefined) {
    return toHexString(byteArray["@@TAGGED@@"][1]);
  }
  return "Unsupported type";
}

export function fromHexString(hexString: string) {
  let res = [];
  for (let i = 0; i < hexString.length; i += 2) {
    res.push(Number("0x" + hexString.substring(i, i + 2)));
  }
  return res;
}

export function substateIdToString(substateId: SubstateId | string | null | undefined) {
  if (substateId === null || substateId === undefined) {
    return "";
  }
  if (typeof substateId === "string") {
    return substateId;
  }
  const key = Object.keys(substateId)[0] as keyof SubstateId;
  return substateId[key];
}

export function shortenSubstateId(
  substateId: SubstateId | string | null | undefined,
  start: number = 4,
  end: number = 4,
) {
  if (substateId === null || substateId === undefined) {
    return "";
  }
  const string = substateIdToString(substateId);
  const parts = string.split("_", 2);
  return parts[0] + "_" + shortenString(parts[1], start, end);
}

export function shortenString(string: string | null | undefined, start: number = 8, end: number = 8) {
  if (string === null || string === undefined) {
    return "";
  }
  // The number 3 is from the characters for ellipsis
  if (string.length < start + end + 3) {
    return string;
  }
  return string.substring(0, start) + "..." + string.slice(-end);
}

export function emptyRows(page: number, rowsPerPage: number, array: Array<any> | undefined) {
  if (array === undefined) {
    return 0;
  }
  return page > 0 ? Math.max(0, (1 + page) * rowsPerPage - array.length) : 0;
}

export function handleChangePage(
  event: unknown,
  newPage: number,
  setPage: React.Dispatch<React.SetStateAction<number>>,
) {
  setPage(newPage);
}

export function handleChangeRowsPerPage(
  event: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>,
  setRowsPerPage: React.Dispatch<React.SetStateAction<number>>,
  setPage: React.Dispatch<React.SetStateAction<number>>,
) {
  setRowsPerPage(parseInt(event.target.value, 10));
  setPage(0);
}

// Converts an ArrayBuffer directly to base64, without any intermediate 'convert to string then
// use window.btoa' step. According to my tests, this appears to be a faster approach:
// http://jsperf.com/encoding-xhr-image-data/5

/*
MIT LICENSE
Copyright 2011 Jon Leighton
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

export function base64FromArrayBuffer(arrayBuffer: ArrayBuffer) {
  let base64 = "";
  const ENCODINGS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

  const bytes = new Uint8Array(arrayBuffer);
  const byteLength = bytes.byteLength;
  const byteRemainder = byteLength % 3;
  const mainLength = byteLength - byteRemainder;

  let a, b, c, d;
  let chunk;

  // Main loop deals with bytes in chunks of 3
  for (let i = 0; i < mainLength; i = i + 3) {
    // Combine the three bytes into a single integer
    chunk = (bytes[i] << 16) | (bytes[i + 1] << 8) | bytes[i + 2];

    // Use bitmasks to extract 6-bit segments from the triplet
    a = (chunk & 16515072) >> 18; // 16515072 = (2^6 - 1) << 18
    b = (chunk & 258048) >> 12; // 258048   = (2^6 - 1) << 12
    c = (chunk & 4032) >> 6; // 4032     = (2^6 - 1) << 6
    d = chunk & 63; // 63       = 2^6 - 1

    // Convert the raw binary segments to the appropriate ASCII encoding
    base64 += ENCODINGS[a] + ENCODINGS[b] + ENCODINGS[c] + ENCODINGS[d];
  }

  // Deal with the remaining bytes and padding
  if (byteRemainder == 1) {
    chunk = bytes[mainLength];

    a = (chunk & 252) >> 2; // 252 = (2^6 - 1) << 2

    // Set the 4 least significant bits to zero
    b = (chunk & 3) << 4; // 3   = 2^2 - 1

    base64 += ENCODINGS[a] + ENCODINGS[b] + "==";
  } else if (byteRemainder == 2) {
    chunk = (bytes[mainLength] << 8) | bytes[mainLength + 1];

    a = (chunk & 64512) >> 10; // 64512 = (2^6 - 1) << 10
    b = (chunk & 1008) >> 4; // 1008  = (2^6 - 1) << 4

    // Set the 2 least significant bits to zero
    c = (chunk & 15) << 2; // 15    = 2^4 - 1

    base64 += ENCODINGS[a] + ENCODINGS[b] + ENCODINGS[c] + "=";
  }

  return base64;
}
