// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { Stack, Select, MenuItem, FormControl } from "@mui/material";
import { OutputStatus } from "@tari-project/typescript-bindings";
import CheckRoundedIcon from "@mui/icons-material/CheckRounded";

interface SortableHeaderProps {
  title: string;
  currentFilter: OutputStatus | "all";
  onFilterChange: (status: OutputStatus | "all") => void;
  getDisplayName: (status: OutputStatus | "all") => string;
}

const CheckIcon = () => (
  <CheckRoundedIcon style={{ fontSize: 22, background: "#FFF", color: "#000", borderRadius: "50%", padding: "2px" }} />
);

function SortableHeader({ title, currentFilter, onFilterChange, getDisplayName }: SortableHeaderProps) {
  return (
    <Stack direction="row" alignItems="center" spacing={1}>
      <FormControl>
        <Select
          value={currentFilter}
          onChange={(e) => onFilterChange(e.target.value as OutputStatus | "all")}
          variant="standard"
          disableUnderline
          size="small"
          renderValue={(value) => (
            <span>
              {title}
              {value !== "all" ? `: ${getDisplayName(value)}` : ""}
            </span>
          )}
          sx={{
            "fontSize": "inherit",
            "minWidth": "220px",
            "& .MuiSelect-select": {
              paddingLeft: 0,
              paddingTop: 0,
              paddingBottom: 0,
              backgroundColor: "transparent !important",
            },
            "& .MuiSelect-select:focus": {
              backgroundColor: "transparent !important",
            },
            "& .Mui-focused": {
              backgroundColor: "transparent !important",
            },
          }}
        >
          <MenuItem value="all">
            <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ width: "100%" }}>
              <span>All</span>
              {currentFilter === "all" && <CheckIcon />}
            </Stack>
          </MenuItem>
          <MenuItem value="Unspent">
            <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ width: "100%" }}>
              <span>Unspent</span>
              {currentFilter === "Unspent" && <CheckIcon />}
            </Stack>
          </MenuItem>
          <MenuItem value="Spent">
            <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ width: "100%" }}>
              <span>Spent</span>
              {currentFilter === "Spent" && <CheckIcon />}
            </Stack>
          </MenuItem>
          <MenuItem value="LockedForSpend">
            <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ width: "100%" }}>
              <span>Locked for Spend</span>
              {currentFilter === "LockedForSpend" && <CheckIcon />}
            </Stack>
          </MenuItem>
          <MenuItem value="LockedUnconfirmed">
            <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ width: "100%" }}>
              <span>Locked Unconfirmed</span>
              {currentFilter === "LockedUnconfirmed" && <CheckIcon />}
            </Stack>
          </MenuItem>
          <MenuItem value="Invalid">
            <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ width: "100%" }}>
              <span>Invalid</span>
              {currentFilter === "Invalid" && <CheckIcon />}
            </Stack>
          </MenuItem>
        </Select>
      </FormControl>
    </Stack>
  );
}

export default SortableHeader;
