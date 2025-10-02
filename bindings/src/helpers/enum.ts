//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

export function matchesTypeEnum<T extends Object>(enumObject: T | null, value: T | null): boolean {
  if (enumObject === null && value === null) {
    return true;
  }
  if (enumObject === null || value === null) {
    return false;
  }

  const keys = Object.keys(enumObject);
  if (keys.length !== 1) {
    throw new Error("Enum object must have exactly one key");
  }
  const key = keys[0] as keyof T;
  if (!(key in value)) {
    return false;
  }

  // check the value
  const enumValue = (enumObject as any)[key];
  const valueValue = (value as any)[key];

  // Check for primitive types
  if (typeof enumValue === "string" || typeof enumValue === "number" || typeof enumValue === "boolean") {
    return typeof enumValue === typeof valueValue;
  }

  // Check for object types (shallow check)
  if (typeof enumValue === "object" && enumValue !== null) {
    return enumValue === valueValue;
  }
  return false;
}
