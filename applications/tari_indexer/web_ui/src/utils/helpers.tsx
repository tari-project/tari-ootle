//  Copyright 2025. The Tari Project
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

import type { Amount } from "@tari-project/ootle-ts-bindings";
import { ReactNode } from "react";
import { toHexString } from "../routes/VN/Components/helpers";
import { CURRENCY } from "./constants";

type JSONObj = Record<string, unknown>;
type JSONArr = Array<unknown>;
type JSONRenderItem = JSONArr | JSONObj | string | unknown;

const renderJson = (json: JSONRenderItem) => {
  function handleObj(obj: JSONObj) {
    const mapped = Object.entries(obj).map(([k, v]) => (
      <li key={`${k}-${v}`}>
        <b>"{k}"</b>:{renderJson(v)}
      </li>
    ));
    return (
      <>
        {`{`}
        <ul>{mapped}</ul>
        {`}`}
      </>
    );
  }
  function handleArr(arr: JSONArr) {
    if (arr.length === 32) {
      return <span className="string">"{toHexString(arr as number[])}"</span>;
    }
    const mapped = arr.map((val, i) => <li key={`json-arr-item-${i}`}>{renderJson(val)},</li>);
    return (
      <>
        [<ol>{mapped}</ol>],
      </>
    );
  }

  if (Array.isArray(json)) {
    return handleArr(json as JSONArr);
  }
  if (typeof json === "object" && json !== null) {
    return handleObj(json as JSONObj);
  }
  if (typeof json === "string") return <span className="string">"{json}"</span>;
  return <span className="other">{json as ReactNode}</span>;
};

export interface Duration {
  secs: number;
  nanos: number;
}

export function displayDuration(duration: Duration) {
  if (duration.secs === 0) {
    if (duration.nanos > 1000000) {
      return `${(duration.nanos / 1000000).toFixed(2)}ms`;
    }
    if (duration.nanos > 1000) {
      return `${(duration.nanos / 1000).toFixed(2)}µs`;
    }
    return `${duration.nanos}ns`;
  }
  if (duration.secs >= 60 * 60) {
    const minutes_secs = duration.secs - Math.floor(duration.secs / 60 / 60) * 60 * 60;
    return `${(duration.secs / 60 / 60).toFixed(0)}h${Math.floor(minutes_secs / 60)}m`;
  }
  if (duration.secs >= 60) {
    const secs = duration.secs - Math.floor(duration.secs / 60) * 60;
    return `${(duration.secs / 60).toFixed(0)}m${secs.toFixed(0)}s`;
  }
  return `${duration.secs}s`;
}

export function truncateText(text: string | null, length: number) {
  if (!length || !text || text.length <= length) {
    return text;
  }
  if (text.length <= length) {
    return text;
  }
  const leftChars = Math.ceil(length / 2);
  const rightChars = Math.floor(length / 2);
  return text.substring(0, leftChars) + "..." + text.substring(text.length - rightChars);
}

const validateHash = (hash: string): boolean => {
  // Hash should be exactly 64 characters long and contain only hexadecimal characters
  const hashRegex = /^[a-fA-F0-9]{64}$/;
  return hashRegex.test(hash);
};

// formatTimestamp.ts
const formatTimestamp = (rawTimestamp: string | null | undefined): string => {
  if (!rawTimestamp) return "";

  let formatted = rawTimestamp;

  // If it doesn't already have "T" between date and time, add it
  if (!formatted.includes("T")) {
    formatted = formatted.replace(" ", "T");
  }

  // If it ends with ".0", remove it
  if (formatted.endsWith(".0")) {
    formatted = formatted.slice(0, -2);
  }

  // If it doesn't already end with "Z" or have a timezone offset, add Z for UTC
  if (!/[Z+\-]\d{2}:?\d{2}$/.test(formatted)) {
    formatted += "Z";
  }

  const date = new Date(formatted);

  if (isNaN(date.getTime())) return "";

  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
};

const parseTimestamp = (rawTimestamp: string | null | undefined): Date | null => {
  if (!rawTimestamp) return null;

  let formatted = rawTimestamp;

  if (!formatted.includes("T")) {
    formatted = formatted.replace(" ", "T");
  }

  if (formatted.endsWith(".0")) {
    formatted = formatted.slice(0, -2);
  }

  if (!/[Z+\-]\d{2}:?\d{2}$/.test(formatted)) {
    formatted += "Z";
  }

  const date = new Date(formatted);
  return isNaN(date.getTime()) ? null : date;
};

const isTimestampNew = (timestamp: string | null | undefined): boolean => {
  const date = parseTimestamp(timestamp);
  if (!date) return false;

  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMinutes = Math.floor(diffMs / (1000 * 60));

  return diffMinutes <= 10;
};

const formatXTM = (amount: number | bigint): string => {
  if (typeof amount !== "number" || isNaN(amount)) {
    return `0 ${CURRENCY.SYMBOL}`;
  }
  return `${(amount / CURRENCY.DIVISOR).toFixed(CURRENCY.DECIMALS)} ${CURRENCY.SYMBOL}`;
};

export { formatTimestamp, formatXTM, isTimestampNew, parseTimestamp, renderJson, validateHash };

export function bigintToDecimalString(int: Amount, decimalPlaces: number, locale: string = "en-US"): string {
  const number = typeof int === "bigint" ? int : BigInt(int);

  if (decimalPlaces == 0) {
    return number.toLocaleString(locale, {
      minimumFractionDigits: 0,
      maximumFractionDigits: 0,
    });
  }
  const wholeValues = (number / BigInt(10 ** decimalPlaces)).toLocaleString(locale, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  });
  const fractionalValues = number.toString().slice(-decimalPlaces);
  if (wholeValues === "0" && fractionalValues === "0") {
    return "0";
  }

  const padding = "0".repeat(decimalPlaces - fractionalValues.length);
  return `${wholeValues}.${padding}${fractionalValues}`;
}

// Helper function for formatting currency amounts
export const formatCurrency = (
  amount: Amount | null | undefined,
  divisibility: number,
  tokenSymbol: string | null | undefined,
): string => {
  const currencySymbol = tokenSymbol ?? "";
  if (!amount) {
    return `0 ${currencySymbol}`;
  }
  try {
    return `${bigintToDecimalString(amount, divisibility)} ${currencySymbol}`;
  } catch (error) {
    console.error("Failed to parse Amount:", amount, error);
    return `-- ${currencySymbol}`;
  }
};
