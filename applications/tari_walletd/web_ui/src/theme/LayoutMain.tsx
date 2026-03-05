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

import { breadcrumbRoutes } from "@/App";
import Logo from "@assets/Logo";
import Breadcrumbs from "@components/Breadcrumbs";
import MenuItems from "@components/MenuItems";
import { Check } from "@mui/icons-material";
import MenuOpenOutlinedIcon from "@mui/icons-material/MenuOpenOutlined";
import MenuOutlinedIcon from "@mui/icons-material/MenuOutlined";
import { Dialog, ThemeProvider } from "@mui/material";
import MuiAppBar, { AppBarProps as MuiAppBarProps } from "@mui/material/AppBar";
import Box from "@mui/material/Box";
import Container from "@mui/material/Container";
import CssBaseline from "@mui/material/CssBaseline";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import MuiDrawer from "@mui/material/Drawer";
import Grid from "@mui/material/Grid";
import IconButton from "@mui/material/IconButton";
import List from "@mui/material/List";
import { createTheme, styled } from "@mui/material/styles";
import Toolbar from "@mui/material/Toolbar";
import useAccountStore from "@store/accountStore";
import useAuthStore from "@store/authStore";
import useSettingsStore from "@store/settingsStore";
import useThemeStore from "@store/themeStore";
import { lightAlpha } from "@theme/colors";
import { componentSettings, dark, light } from "@theme/tokens";
import { settingsGet } from "@utils/json_rpc";
import { useEffect, useState } from "react";
import { Link, Outlet } from "react-router";
import "./theme.css";

const DRAWER_WIDTH = 300;

interface AppBarProps extends MuiAppBarProps {
  open?: boolean;
}

const AppBar = styled(MuiAppBar, {
  shouldForwardProp: (prop) => prop !== "open",
})<AppBarProps>(({ theme, open }) => ({
  zIndex: theme.zIndex.drawer + 1,
  transition: theme.transitions.create(["width", "margin"], {
    easing: theme.transitions.easing.easeOut,
    duration: theme.transitions.duration.enteringScreen,
  }),
  ...(open && {
    marginLeft: DRAWER_WIDTH,
    width: `calc(100% - ${DRAWER_WIDTH}px)`,
    transition: theme.transitions.create(["width", "margin"], {
      easing: theme.transitions.easing.easeOut,
      duration: theme.transitions.duration.enteringScreen,
    }),
  }),
}));

const Drawer = styled(MuiDrawer, {
  shouldForwardProp: (prop) => prop !== "open",
})(({ theme, open }) => ({
  "& .MuiDrawer-paper": {
    position: "relative",
    whiteSpace: "nowrap",
    borderRight: `1px solid ${lightAlpha[5]}`,
    boxShadow: "10px 14px 28px rgb(35 11 73 / 5%)",
    width: DRAWER_WIDTH,
    transition: theme.transitions.create("width", {
      easing: theme.transitions.easing.easeOut,
      duration: theme.transitions.duration.enteringScreen,
    }),
    boxSizing: "border-box",
    ...(!open && {
      overflowX: "hidden",
      transition: theme.transitions.create("width", {
        easing: theme.transitions.easing.easeOut,
        duration: theme.transitions.duration.leavingScreen,
      }),
      width: theme.spacing(7),
      [theme.breakpoints.up("sm")]: {
        width: theme.spacing(9),
      },
    }),
  },
}));

export default function Layout() {
  const [open, setOpen] = useState(false);
  const { themeMode } = useThemeStore();
  const popup = useAccountStore((state) => state.popup);
  const setPopup = useAccountStore((state) => state.setPopup);
  const { loggedIn } = useAuthStore();
  const setAdvancedUiFeatures = useSettingsStore((s) => s.setAdvancedUiFeatures);

  useEffect(() => {
    settingsGet().then((res) => {
      setAdvancedUiFeatures(res.advanced_ui_features);
    });
  }, [setAdvancedUiFeatures]);

  const handleClose = () => {
    setPopup({ visible: false });
  };
  const toggleDrawer = () => {
    setOpen(!open);
  };

  const themeOptions = (mode: string) => {
    return mode === "light" ? light : dark;
  };

  const theme = createTheme({
    ...themeOptions(themeMode),
    ...componentSettings,
  });

  return (
    <ThemeProvider theme={theme}>
      <Dialog open={popup?.visible ?? false} onClose={handleClose}>
        <DialogTitle style={{ color: popup?.error ? theme.palette.error.main : theme.palette.success.main }}>
          {popup?.error ? null : <Check style={{ color: "green" }} />}
          {popup?.title}
        </DialogTitle>
        <DialogContent className="dialog-content">{popup?.message}</DialogContent>
      </Dialog>
      <Box sx={{ display: "flex" }}>
        <CssBaseline />
        <AppBar position="absolute" open={open} elevation={0}>
          <Toolbar
            sx={{
              pr: "24px",
            }}
          >
            <IconButton
              edge="start"
              color="inherit"
              aria-label="open drawer"
              onClick={toggleDrawer}
              sx={{
                marginRight: "36px",
                color: "#757575",
                ...(open && { display: "none" }),
              }}
            >
              <MenuOutlinedIcon />
            </IconButton>
            <Box
              style={{
                display: "flex",
                justifyContent: "space-between",
                width: "100%",
                alignItems: "center",
              }}
            >
              <Link
                to="/"
                style={{
                  paddingTop: theme.spacing(1),
                }}
              >
                <Logo fill={theme.palette.text.primary} />
              </Link>
              {/*<Stack direction="row" spacing={1}>*/}
              {/*  {loggedIn ? <WalletConnectLink /> : null}*/}
              {/*</Stack>*/}
            </Box>
          </Toolbar>
        </AppBar>
        <Drawer variant="permanent" open={open}>
          <Toolbar
            sx={{
              display: "flex",
              alignItems: "center",
              justifyContent: "flex-end",
              px: [1],
            }}
          >
            <IconButton onClick={toggleDrawer}>
              <MenuOpenOutlinedIcon />
            </IconButton>
          </Toolbar>
          <List component="nav">
            <MenuItems />
          </List>
        </Drawer>
        <Box
          component="main"
          sx={{
            flexGrow: 1,
            height: "100vh",
            overflow: "auto",
          }}
        >
          <Toolbar />
          <Container
            maxWidth="xl"
            style={{
              paddingTop: theme.spacing(3),
              paddingBottom: theme.spacing(5),
            }}
          >
            <Grid container spacing={3}>
              <Grid size={12}>
                <div
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                    borderBottom: `1px solid ${theme.palette.divider}`,
                  }}
                >
                  <Breadcrumbs items={breadcrumbRoutes} />
                </div>
              </Grid>
              <Outlet />
            </Grid>
          </Container>
        </Box>
      </Box>
    </ThemeProvider>
  );
}
