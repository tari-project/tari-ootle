// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { Chip, Avatar } from "@mui/material";
import {
  IoWalletOutline,
  IoTimeOutline,
  IoLockClosedOutline,
  IoArrowForwardOutline,
  IoWarningOutline,
} from "react-icons/io5";
import { useTheme } from "@mui/material/styles";
import type { OutputStatus } from "@tari-project/typescript-bindings";

interface StatusChipProps {
  status: OutputStatus;
  showTitle?: boolean;
}

const colorList: Record<string, string> = {
  Unspent: "#5F9C91",
  Spent: "#ECA86A",
  LockedForSpend: "#318EFA",
  LockedUnconfirmed: "#9D5CF9",
  Invalid: "#DB7E7E",
};

export default function StatusChip({ status, showTitle = true }: StatusChipProps) {
  const theme = useTheme();

  const iconList: Record<string, JSX.Element> = {
    Unspent: <IoWalletOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />,
    Spent: <IoArrowForwardOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />,
    LockedForSpend: <IoLockClosedOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />,
    LockedUnconfirmed: <IoTimeOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />,
    Invalid: <IoWarningOutline style={{ height: 14, width: 14 }} color={theme.palette.background.paper} />,
  };

  let bgColor = colorList[status];
  let background = null;

  if (!showTitle) {
    return <Avatar sx={{ bgcolor: bgColor, height: 22, width: 22 }}>{iconList[status]}</Avatar>;
  } else {
    return (
      <Chip
        avatar={<Avatar sx={{ bgcolor: bgColor, background: background }}>{iconList[status]}</Avatar>}
        label={status}
        style={{ color: colorList[status], borderColor: colorList[status] }}
        variant="outlined"
      />
    );
  }
}
