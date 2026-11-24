//  Copyright 2022. The Tari Project
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

import { Avatar, Chip } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import useMediaQuery from "@mui/material/useMediaQuery";
import type { TransactionStatus } from "@tari-project/ootle-ts-bindings";
import { ReactNode } from "react";
import { IoCheckmarkOutline, IoCloseOutline, IoDiamondOutline, IoHourglassOutline, IoReload } from "react-icons/io5";
import { getTransactionStatusPresentation } from "./transactionsStatusChipHelpers";

interface StatusChipProps {
  status: TransactionStatus;
  showTitle?: boolean;
}

function renderStatusIcon(status: TransactionStatus, iconColor: string): ReactNode {
  switch (status) {
    case "Accepted":
      return <IoCheckmarkOutline style={{ height: 14, width: 14 }} color={iconColor} />;
    case "Pending":
      return <IoHourglassOutline style={{ height: 14, width: 14 }} color={iconColor} />;
    case "DryRun":
      return <IoReload style={{ height: 14, width: 14 }} color={iconColor} />;
    case "New":
      return <IoDiamondOutline style={{ height: 14, width: 14 }} color={iconColor} />;
    case "Rejected":
    case "InvalidTransaction":
      return <IoCloseOutline style={{ height: 14, width: 14 }} color={iconColor} />;
    case "OnlyFeeAccepted":
      return (
        <>
          <IoCheckmarkOutline style={{ height: 14, width: 14 }} color={iconColor} />
          <IoCloseOutline style={{ height: 14, width: 14 }} color={iconColor} />
        </>
      );
    default:
      return <IoHourglassOutline style={{ height: 14, width: 14 }} color={iconColor} />;
  }
}

export default function TransactionsStatusChip({ status, showTitle = true }: StatusChipProps) {
  const theme = useTheme();
  const isSm = useMediaQuery(theme.breakpoints.down("sm"));
  const { color, avatarBackground, label } = getTransactionStatusPresentation(status);
  const icon = renderStatusIcon(status, theme.palette.background.paper);

  if (!showTitle) {
    return (
      <Avatar
        role="img"
        aria-label={label}
        title={label}
        style={{ background: avatarBackground, height: 22, width: 22 }}
      >
        {icon}
      </Avatar>
    );
  }

  return (
    <Chip
      avatar={<Avatar style={{ background: avatarBackground }}>{icon}</Avatar>}
      label={label}
      style={{ color, borderColor: color }}
      variant="outlined"
      size={isSm ? "small" : "medium"}
    />
  );
}
