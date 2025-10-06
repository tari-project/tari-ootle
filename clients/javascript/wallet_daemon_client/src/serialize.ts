//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Allow BigInt to be serialized to JSON (note that deserializing from string is not necessarily supported
// @ts-ignore
BigInt.prototype.toJSON = function () {
  if ((this as bigint) > BigInt(Number.MAX_SAFE_INTEGER)) {
    return this.toString();
  } else {
    return Number(this);
  }
};
