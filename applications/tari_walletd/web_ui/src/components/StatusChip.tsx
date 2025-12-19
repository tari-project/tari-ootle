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
import { IoCheckmarkOutline, IoDiamondOutline, IoReload, IoHourglassOutline, IoCloseOutline } from "react-icons/io5";
import { useTheme } from "@mui/material/styles";

export const StatusChipIcons = {
  Checkmark: "Checkmark",
  DiamondOutline: "DiamondOutline",
  Reload: "Reload",
  HourglassOutline: "HourglassOutline",
  CloseOutline: "CloseOutline",
} as const;

export type StatusChipIcon = (typeof StatusChipIcons)[keyof typeof StatusChipIcons];

export const StatusChipColors = {
  Green: "#5F9C91",
  Yellow: "#ECA86A",
  Blue: "#318EFA",
  Purple: "#9D5CF9",
  Red: "#DB7E7E",
  Orange: "#FFA500",
} as const;

export type StatusChipColor = (typeof StatusChipColors)[keyof typeof StatusChipColors];

interface StatusChipProps {
  icon?: StatusChipIcon;
  children?: React.ReactNode;
  title?: string;
  color?: StatusChipColor;
}

export default function StatusChip({ icon, title, color = StatusChipColors.Green, children }: StatusChipProps) {
  const theme = useTheme();

  let iconJsx;
  if (icon) {
    switch (icon) {
      case StatusChipIcons.Checkmark:
        iconJsx = <IoCheckmarkOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />;
        break;
      case StatusChipIcons.DiamondOutline:
        iconJsx = <IoDiamondOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />;
        break;
      case StatusChipIcons.Reload:
        iconJsx = <IoReload style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />;
        break;
      case StatusChipIcons.HourglassOutline:
        iconJsx = <IoHourglassOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />;
        break;
      case StatusChipIcons.CloseOutline:
        iconJsx = <IoCloseOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />;
        break;
    }
  }

  let background = null;

  if (children) {
    return (
      <Chip
        avatar={iconJsx ? <Avatar sx={{ bgcolor: color, background: background }}>{iconJsx}</Avatar> : undefined}
        label={children}
        style={{ color: color, borderColor: color }}
        variant="outlined"
        title={title}
      />
    );
  } else {
    return <Avatar sx={{ bgcolor: color, height: 22, width: 22 }}>{iconJsx}</Avatar>;
  }
}
