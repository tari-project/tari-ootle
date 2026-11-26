//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import type { Type } from "@tari-project/ootle-ts-bindings";

export function formatType(funcType: Type): string {
  if (typeof funcType === "string") {
    return funcType;
  }
  if ("Vec" in funcType) {
    return `Vec<${formatType(funcType.Vec)}>`;
  }
  if ("Option" in funcType) {
    return `Option<${formatType(funcType.Option)}>`;
  }
  if ("Tuple" in funcType) {
    return `(${funcType.Tuple.map(formatType).join(", ")})`;
  }
  if ("Other" in funcType) {
    return funcType.Other.name;
  }
  if ("WellKnown" in funcType) {
    return funcType.WellKnown;
  }
  return "Unknown";
}
