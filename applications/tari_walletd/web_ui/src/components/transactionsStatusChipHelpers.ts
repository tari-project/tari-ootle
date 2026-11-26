//  Copyright 2026. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import type { TransactionStatus } from "@tari-project/ootle-ts-bindings";

export interface TransactionStatusPresentation {
  color: string;
  avatarBackground: string;
  label: string;
}

const DEFAULT_STATUS_COLOR = "#757575";

export const TRANSACTION_STATUS_COLORS: Record<TransactionStatus, string> = {
  Accepted: "#5F9C91",
  Pending: "#ECA86A",
  DryRun: "#318EFA",
  New: "#9D5CF9",
  Rejected: "#DB7E7E",
  InvalidTransaction: "#DB7E7E",
  OnlyFeeAccepted: "#FFA500",
};

export function getTransactionStatusPresentation(status: TransactionStatus): TransactionStatusPresentation {
  const color = TRANSACTION_STATUS_COLORS[status] ?? DEFAULT_STATUS_COLOR;
  const avatarBackground =
    status === "OnlyFeeAccepted"
      ? `linear-gradient(to right, ${TRANSACTION_STATUS_COLORS.Accepted} 50%, ${TRANSACTION_STATUS_COLORS.Rejected} 50%)`
      : color;

  return {
    color,
    avatarBackground,
    label: status,
  };
}
