//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import { Memo as TyMemo } from "@tari-project/typescript-bindings";

export type MemoProps = {
  memo?: TyMemo | null;
};

export function Memo({ memo }: MemoProps) {
  if (!memo) {
    return <span>--</span>;
  }

  if ("Message" in memo) {
    return <span>{memo ? memo.Message : "No Memo"}</span>;
  }
  if ("Bytes" in memo) {
    return <span>{memo ? Buffer.from(memo.Bytes).toString("hex") : "No Memo"}</span>;
  }

  return <span>{JSON.stringify(memo)}</span>;
}
