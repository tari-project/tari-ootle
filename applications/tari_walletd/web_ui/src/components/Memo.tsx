//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import CopyToClipboard from "@components/CopyToClipboard";
import StatusChip, { StatusChipColors } from "@components/StatusChip";
import { Memo as TyMemo } from "@tari-project/ootle-ts-bindings";
import { decodeDestinationTagAndBytes, formatMemoBytes } from "./memoHelpers";

export type MemoProps = {
  memo?: TyMemo | null;
};

export function Memo({ memo }: MemoProps) {
  if (!memo) {
    return <span>--</span>;
  }

  if ("Message" in memo) {
    return <span>{memo.Message}</span>;
  }
  if ("Bytes" in memo) {
    return <span>{formatMemoBytes(memo.Bytes)}</span>;
  }
  if ("U256" in memo) {
    return <span>{memo.U256}</span>;
  }
  if ("SenderAddress" in memo) {
    // The resolved Ootle address is surfaced separately (see StealthUtxoList); show a label if we end up here.
    return <span>Sender address</span>;
  }
  if ("DestinationTagAndBytes" in memo) {
    const decoded = decodeDestinationTagAndBytes(memo.DestinationTagAndBytes);
    if (decoded) {
      const { destinationTag, message } = decoded;
      return (
        <span>
          {message}
          {destinationTag && (
            <span style={message ? { marginLeft: "8px" } : undefined}>
              <StatusChip color={StatusChipColors.Blue} title="This UTXO has an attached destination tag">
                <CopyToClipboard copy={destinationTag} /> {destinationTag}
              </StatusChip>
            </span>
          )}
        </span>
      );
    }
  }

  return <span>{JSON.stringify(memo)}</span>;
}
