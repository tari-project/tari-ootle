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

import { Chip, Avatar } from "@mui/material";
import { IoCheckmarkOutline, IoCloseOutline } from "react-icons/io5";
import { useTheme } from "@mui/material/styles";
import type { Decision } from "@tari-project/ootle-ts-bindings";
import {ReactNode} from "react";

interface StatusChipProps {
  status: Decision;
  showTitle?: boolean;
}

const colorList: Record<string, string> = {
  Commit: "#5F9C91",
  Abort: "#DB7E7E",
  // Pending: '#ECA86A',
  // DryRun: '#318EFA',
  // New: '#9D5CF9',
  // InvalidTransaction: '#DB7E7E',
  // OnlyFeeAccepted: '#FFA500',
};

export default function StatusChip({
  status,
  showTitle = true,
}: StatusChipProps) {
  const theme = useTheme();

  const statusKey = typeof status === "string" ? status : "Abort";
  const statusLabel =
    typeof status === "string" ? status : `Abort: ${status.Abort}`;

  const iconList: Record<string, ReactNode> = {
    Commit: (
      <IoCheckmarkOutline
        style={{ height: 14, width: 14 }}
        color={theme.palette.background.paper}
      />
    ),
    Abort: (
      <IoCloseOutline
        style={{ height: 14, width: 14 }}
        color={theme.palette.background.paper}
      />
    ),

  };

  let bgColor = colorList[statusKey];
  let background = null;

  if (!showTitle) {
    return (
      <Avatar sx={{ bgcolor: bgColor, height: 22, width: 22 }}>
        {iconList[statusKey]}
      </Avatar>
    );
  } else {
    return (
      <Chip
        avatar={
          <Avatar sx={{ bgcolor: bgColor, background: background }}>
            {iconList[statusKey]}
          </Avatar>
        }
        label={statusLabel}
        style={{
          color: colorList[statusKey],
          borderColor: colorList[statusKey],
        }}
        variant="outlined"
      />
    );
  }
}
