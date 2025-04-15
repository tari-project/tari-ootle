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

import { NavLink } from "react-router-dom";
import ListItemButton from "@mui/material/ListItemButton";
import ListItemIcon from "@mui/material/ListItemIcon";
import ListItemText from "@mui/material/ListItemText";
import { IoHome, IoHomeOutline } from "react-icons/io5";
import Tooltip from "@mui/material/Tooltip";
import Fade from "@mui/material/Fade";
import ThemeSwitcher from "./ThemeSwitcher";
import Box from "@mui/material/Box";
import { useTheme } from "@mui/material/styles";
import { useDatabasesList } from "../store/databases.ts";
import { GiOilDrum } from "react-icons/gi";
import { OilBarrel } from "@mui/icons-material";

function MainListItems() {
  const theme = useTheme();


  const { data: databases, isLoading, error, status } = useDatabasesList();

  if (status === "error") {
    return (
      <div>
        <p>Error loading databases: {error.message}</p>
      </div>
    );
  }

  if (isLoading || !databases) {
    return <div>Loading...</div>;
  }

  const iconStyle = {
    height: 22,
    width: 22,
  };

  const activeIconStyle = {
    height: 22,
    width: 22,
    color: theme.palette.primary.main,
  };

  const databaseItems = databases.map((db) => ({
    title: db.name,
    icon: <GiOilDrum style={iconStyle} />,
    activeIcon: <OilBarrel style={activeIconStyle} />,
    link: `/databases/${encodeURIComponent(db.name)}`,
  }));

  const mainItems = [
    {
      title: "Home",
      icon: <IoHomeOutline style={iconStyle} />,
      activeIcon: <IoHome style={activeIconStyle} />,
      link: "/",
    },
    ...databaseItems,
    // {
    //   title: "Templates",
    //   icon: <LuLayoutTemplate style={iconStyle} />,
    //   activeIcon: <LuLayoutTemplate style={activeIconStyle} />,
    //   link: "templates",
    // },
    // {
    //   title: "Manifest",
    //   icon: <IoTerminalOutline style={iconStyle} />,
    //   activeIcon: <IoTerminal style={activeIconStyle} />,
    //   link: "manifest",
    // },
    // {
    //   title: "Settings",
    //   icon: <IoSettingsOutline style={iconStyle} />,
    //   activeIcon: <IoSettings style={activeIconStyle} />,
    //   link: "settings",
    // },
  ];


  return (
    <Box
      style={{
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
        height: "calc(100vh - 84px)",
      }}
    >
      <Box>
        {mainItems.map(({ title, icon, activeIcon, link }, i) => (
          <NavLink
            key={i}
            to={link}
            style={{
              textDecoration: "none",
              color: "inherit",
            }}
          >
            {({ isActive }) => (
              <ListItemButton
                sx={{
                  paddingLeft: "22px",
                  paddingRight: "22px",
                }}
              >
                <Tooltip
                  slots={{ transition: Fade }}
                  slotProps={{ transition: { timeout: 300 } }}
                  title={title}
                  followCursor={false}
                  placement="right"
                  arrow
                >
                  <ListItemIcon>{isActive ? activeIcon : icon}</ListItemIcon>
                </Tooltip>
                <ListItemText secondary={title} />
              </ListItemButton>
            )}
          </NavLink>
        ))}
      </Box>
      <ThemeSwitcher />
    </Box>
  );
}

export default MainListItems;
