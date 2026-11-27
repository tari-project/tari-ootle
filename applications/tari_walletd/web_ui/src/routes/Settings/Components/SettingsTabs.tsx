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

import Box from "@mui/material/Box";
import Divider from "@mui/material/Divider";
import { useTheme } from "@mui/material/styles";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import Typography from "@mui/material/Typography";
import * as React from "react";
import { ISettingsMenu } from "../Settings";
import { resolveActiveTabIndex } from "./settingsTabsHelpers";

interface TabPanelProps {
  children?: React.ReactNode;
  index: number;
  value: number | false;
}

function CustomTabPanel(props: TabPanelProps) {
  const { children, value, index, ...other } = props;

  return (
    <div
      role="tabpanel"
      hidden={value !== index}
      id={`simple-tabpanel-${index}`}
      aria-labelledby={`simple-tab-${index}`}
      {...other}
    >
      {value === index && (
        <Box sx={{ p: 3 }}>
          <Box>{children}</Box>
        </Box>
      )}
    </div>
  );
}

function a11yProps(index: number) {
  return {
    "id": `simple-tab-${index}`,
    "aria-controls": `simple-tabpanel-${index}`,
  };
}

export default function SettingsTabs({ menuItems }: { menuItems: ISettingsMenu[] }) {
  const [selectedLabel, setSelectedLabel] = React.useState<string | null>(menuItems[0]?.label ?? null);
  const theme = useTheme();
  const activeIndex = resolveActiveTabIndex(menuItems, selectedLabel);

  const handleChange = (_event: React.SyntheticEvent, newValue: number) => {
    const nextItem = menuItems[newValue];
    if (nextItem) {
      setSelectedLabel(nextItem.label);
    }
  };

  const menuLabels = menuItems.map((item, index) => {
    return <Tab label={item.label} {...a11yProps(index)} key={item.label} style={{ minWidth: 150 }} />;
  });

  const menuContents = menuItems.map((item, index) => {
    return (
      <CustomTabPanel value={activeIndex} index={index} key={item.label}>
        <Typography
          variant="h5"
          style={{
            paddingTop: theme.spacing(1),
            paddingBottom: theme.spacing(2),
          }}
        >
          {item.title}
        </Typography>
        <Divider />
        {item.content}
      </CustomTabPanel>
    );
  });

  return (
    <Box sx={{ width: "100%" }}>
      <Box sx={{ borderBottom: 1, borderColor: "divider" }}>
        <Tabs
          value={activeIndex}
          onChange={handleChange}
          aria-label="settings tabs"
          variant="scrollable"
          scrollButtons="auto"
        >
          {menuLabels}
        </Tabs>
      </Box>
      {menuContents}
    </Box>
  );
}
