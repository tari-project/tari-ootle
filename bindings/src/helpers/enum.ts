//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

function equalNumberAndBigInt(num: number, big: bigint): boolean {
  return Number.isInteger(num) && BigInt(num) === big;
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) {
    return true;
  }
  if (typeof a === "number" && typeof b === "bigint") {
    return equalNumberAndBigInt(a, b);
  }
  if (typeof a === "bigint" && typeof b === "number") {
    return equalNumberAndBigInt(b, a);
  }
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") {
    return false;
  }
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) {
      return false;
    }
    return a.every((item, idx) => deepEqual(item, b[idx]));
  }
  if (ArrayBuffer.isView(a) || ArrayBuffer.isView(b)) {
    if (!ArrayBuffer.isView(a) || !ArrayBuffer.isView(b) || a.constructor !== b.constructor) {
      return false;
    }
    if (a instanceof DataView && b instanceof DataView) {
      if (a.byteLength !== b.byteLength) {
        return false;
      }
      for (let i = 0; i < a.byteLength; i++) {
        if (a.getUint8(i) !== b.getUint8(i)) {
          return false;
        }
      }
      return true;
    }
    const viewA = a as unknown as ArrayLike<unknown>;
    const viewB = b as unknown as ArrayLike<unknown>;
    if (viewA.length !== viewB.length) {
      return false;
    }
    for (let i = 0; i < viewA.length; i++) {
      if (viewA[i] !== viewB[i]) {
        return false;
      }
    }
    return true;
  }
  const keysA = Object.keys(a);
  const keysB = Object.keys(b);
  if (keysA.length !== keysB.length) {
    return false;
  }
  return keysA.every(
    (key) =>
      Object.prototype.hasOwnProperty.call(b, key) &&
      deepEqual((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]),
  );
}

export function matchesTypeEnum<T extends Object>(enumObject: T | null, value: T | null): boolean {
  if (enumObject === null && value === null) {
    return true;
  }
  if (enumObject === null || value === null) {
    return false;
  }
  if (typeof enumObject === "string") {
    return enumObject === value;
  }
  if (typeof enumObject !== "object" || Array.isArray(enumObject)) {
    throw new Error("Enum object must be a string unit variant or a single-key object");
  }

  const keys = Object.keys(enumObject);
  if (keys.length !== 1) {
    throw new Error("Enum object must have exactly one key");
  }
  if (typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  if (Object.keys(value).length !== 1) {
    return false;
  }
  const key = keys[0];
  if (!Object.prototype.hasOwnProperty.call(value, key)) {
    return false;
  }

  const enumValue = (enumObject as Record<string, unknown>)[key];
  const valueValue = (value as Record<string, unknown>)[key];

  return deepEqual(enumValue, valueValue);
}
