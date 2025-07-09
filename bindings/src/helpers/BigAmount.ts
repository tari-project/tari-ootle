/*
 * //   Copyright 2025 The Tari Project
 * //   SPDX-License-Identifier: BSD-3-Clause
 */

/**
 * Class that represents an arbitrary precision value.
 * This encodes to JSON using the "special" syntax to coerce a value to an Amount.
 */
export default class BigAmount {
  private inner: BigInt;

  constructor(amount: bigint | string | number) {
    this.inner = BigInt(amount);
  }

  static from(amount: bigint | string | number): BigAmount {
    return new BigAmount(amount);
  }

  public get value(): BigInt {
    return this.inner;
  }

  public toString(): string {
    return this.inner.toString();
  }

  public toJSON(): string {
    // Uses the special syntax supported by the wallet/indexer to coerce the value to an Amount
    return `Amount(${this.toString()})`;
  }
}
