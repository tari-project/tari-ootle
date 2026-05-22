// Copyright 2022. The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

// The daemon's Permission JSON shape is direct enough to render without
// wrapping in TS classes — pass the parsed JSON straight through and use
// `permissionToString` from the bindings package for display.

import { permissionToString, type Permission } from "@tari-project/ootle-ts-bindings";

export function parse(permission: unknown): Permission {
  return permission as Permission;
}

export { permissionToString };
