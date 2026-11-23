//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) {
    return true;
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
    if (!(a instanceof Uint8Array) || !(b instanceof Uint8Array) || a.length !== b.length) {
      return false;
    }
    return a.every((byte, idx) => byte === b[idx]);
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
    throw new Error("Enum object must have exactly one key");
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
