//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { Instruction, InstructionArg } from "@tari-project/ootle-ts-bindings";
import { encode, Tag } from "cbor2";
import { describe, expect, it } from "vitest";
import { describeInstructions } from "./describe";

const RESOURCE_ADDRESS_TAG = 131;
const RESOURCE_HEX = "ab".repeat(32);
const RESOURCE = `resource_${RESOURCE_HEX}`;
const OWN_ACCOUNT = `component_${"01".repeat(32)}`;
const OTHER_ACCOUNT = `component_${"02".repeat(32)}`;
const OWN = new Set([OWN_ACCOUNT]);

const hex = (bytes: Uint8Array) => Buffer.from(bytes).toString("hex");
const lit = (value: unknown): InstructionArg => ({ Literal: hex(encode(value)) });
const resourceArg = () => lit(new Tag(RESOURCE_ADDRESS_TAG, Uint8Array.from(Buffer.from(RESOURCE_HEX, "hex"))));
/// `Amount`'s canonical CBOR encoding: its `[low, high]` u64 digits.
const amountArg = (n: number) => lit([n, 0]);
const bucket = (id: number): InstructionArg => ({ Workspace: { id, offset: null } });

const call = (address: string, method: string, args: InstructionArg[]): Instruction => ({
  CallMethod: { call: { Address: address }, method, args },
});
const putOnWorkspace = (key: number): Instruction => ({ PutLastInstructionOutputOnWorkspace: { key } });

describe("describeInstructions", () => {
  it("describes a withdrawal from an own account and the deposit that receives it", () => {
    const instructions = [
      call(OWN_ACCOUNT, "withdraw", [resourceArg(), amountArg(1_000_000)]),
      putOnWorkspace(0),
      call(OTHER_ACCOUNT, "deposit", [bucket(0)]),
    ];

    expect(describeInstructions(instructions, [], OWN)).toEqual({
      withdrawals: [
        {
          account: OWN_ACCOUNT,
          resource: RESOURCE,
          value: { kind: "amount", amount: 1_000_000n },
          destination: { kind: "deposit", component: OTHER_ACCOUNT, isOwnAccount: false },
        },
      ],
      fees: [],
      undescribed: 0,
    });
  });

  it("reads a withdrawal amount carried in a blob", () => {
    const blob = Buffer.from(encode([42, 0])).toString("base64");
    const instructions = [call(OWN_ACCOUNT, "withdraw", [resourceArg(), { Blob: 0 }]), putOnWorkspace(0)];

    const { withdrawals } = describeInstructions(instructions, [blob], OWN);

    expect(withdrawals).toHaveLength(1);
    expect(withdrawals[0].value).toEqual({ kind: "amount", amount: 42n });
  });

  it("counts a template call as undescribed", () => {
    const templateCall: Instruction = { CallFunction: { address: "cd".repeat(32), function: "anything", args: [] } };

    expect(describeInstructions([templateCall], [], OWN)).toEqual({ withdrawals: [], fees: [], undescribed: 1 });
  });

  it("counts a withdrawal from an account outside the wallet as undescribed", () => {
    const instructions = [
      call(OTHER_ACCOUNT, "withdraw", [resourceArg(), amountArg(5)]),
      putOnWorkspace(0),
      call(OWN_ACCOUNT, "deposit", [bucket(0)]),
    ];

    expect(describeInstructions(instructions, [], OWN)).toEqual({ withdrawals: [], fees: [], undescribed: 3 });
  });

  it("leaves the destination unknown when part of the bucket is taken before the deposit", () => {
    const instructions: Instruction[] = [
      call(OWN_ACCOUNT, "withdraw", [resourceArg(), amountArg(1_000_000)]),
      putOnWorkspace(0),
      { TakeFromBucket: { input_bucket: { id: 0, offset: null }, amount: 999_999, output_bucket: 1 } },
      call(OWN_ACCOUNT, "deposit", [bucket(0)]),
      call(OTHER_ACCOUNT, "deposit", [bucket(1)]),
    ];

    const { withdrawals, undescribed } = describeInstructions(instructions, [], OWN);

    expect(withdrawals.map((w) => w.destination)).toEqual([{ kind: "unknown" }]);
    expect(undescribed).toBe(3);
  });

  it("leaves the destination unknown when another call reads the bucket before the deposit", () => {
    const instructions = [
      call(OWN_ACCOUNT, "withdraw", [resourceArg(), amountArg(5)]),
      putOnWorkspace(0),
      call(OTHER_ACCOUNT, "inspect", [bucket(0)]),
      call(OWN_ACCOUNT, "deposit", [bucket(0)]),
    ];

    const { withdrawals, undescribed } = describeInstructions(instructions, [], OWN);

    expect(withdrawals.map((w) => w.destination)).toEqual([{ kind: "unknown" }]);
    expect(undescribed).toBe(2);
  });

  it("describes a withdrawn bucket that pays the fee", () => {
    const instructions: Instruction[] = [
      call(OWN_ACCOUNT, "withdraw", [resourceArg(), amountArg(10)]),
      putOnWorkspace(0),
      { PayFeeFromBucket: { bucket: { id: 0, offset: null } } },
    ];

    const { withdrawals, undescribed } = describeInstructions(instructions, [], OWN);

    expect(withdrawals.map((w) => w.destination)).toEqual([{ kind: "fee" }]);
    expect(undescribed).toBe(0);
  });

  it("counts a withdrawal with an unreadable amount as undescribed", () => {
    const instructions = [call(OWN_ACCOUNT, "withdraw", [resourceArg(), lit("not an amount")])];

    expect(describeInstructions(instructions, [], OWN)).toEqual({ withdrawals: [], fees: [], undescribed: 1 });
  });

  it("describes a fee paid from an own account", () => {
    const instructions = [call(OWN_ACCOUNT, "pay_fee", [amountArg(2_000)])];

    expect(describeInstructions(instructions, [], OWN)).toEqual({
      withdrawals: [],
      fees: [{ account: OWN_ACCOUNT, amount: 2_000n }],
      undescribed: 0,
    });
  });
});
