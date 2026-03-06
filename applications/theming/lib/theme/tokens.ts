//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import { ThemeOptions } from "@mui/material/styles";
import { blue, gothic, green, grey, orange, red, tariBg, tariPurple, teal } from "./colors";
import PoppinsBoldTTF from "/fonts/poppins/Poppins-Bold.ttf";
import PoppinsMediumTTF from "/fonts/poppins/Poppins-Medium.ttf";
import PoppinsRegularTTF from "/fonts/poppins/Poppins-Regular.ttf";
import PoppinsSemiBoldTTF from "/fonts/poppins/Poppins-SemiBold.ttf";

const componentSettings: ThemeOptions = {
  shape: {
    borderRadius: 20,
  },
  spacing: 8,
  typography: {
    fontFamily: '"PoppinsMedium", sans-serif',
    fontSize: 12,
    body1: {
      lineHeight: 1.1,
      letterSpacing: "0.5px",
      fontWeight: 500,
      fontFamily: '"PoppinsMedium", sans-serif',
    },
    body2: {
      lineHeight: "1.5rem",
      fontWeight: 500,
      fontFamily: '"PoppinsMedium", sans-serif',
    },
    h1: {
      fontSize: "2.2rem",
      lineHeight: "3.2rem",
      fontWeight: 700,
      fontFamily: '"PoppinsBold", sans-serif',
    },
    h2: {
      fontSize: "1.9rem",
      lineHeight: "2.9rem",
      fontWeight: 700,
      fontFamily: '"PoppinsBold", sans-serif',
    },
    h3: {
      fontSize: "1.6rem",
      lineHeight: "2.6rem",
      fontWeight: 700,
      fontFamily: '"PoppinsBold", sans-serif',
    },
    h4: {
      fontSize: "1.3rem",
      lineHeight: "2.3rem",
      fontWeight: 600,
      fontFamily: '"PoppinsSemiBold", sans-serif',
    },
    h5: {
      fontSize: "14px",
      lineHeight: "1.4rem",
      fontWeight: 600,
      fontFamily: '"PoppinsSemiBold", sans-serif',
    },
    h6: {
      fontSize: "0.75rem",
      lineHeight: "1.8rem",
      fontWeight: 600,
      fontFamily: '"PoppinsSemiBold", sans-serif',
    },
    // custom
    label: {
      fontSize: 10,
      fontWeight: 400,
      lineHeight: "10px",
      letterSpacing: 0.1,
      fontFamily: '"PoppinsRegular", sans-serif',
    },
    span: {
      lineHeight: 1,
    },
  },
  transitions: {
    duration: {
      enteringScreen: 500,
      leavingScreen: 500,
    },
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: `
        @font-face {
          font-family: "PoppinsRegular";
          src: local('Poppins'), url(${PoppinsRegularTTF}) format('ttf');
          font-display: swap;
          font-weight: 400
        }
        @font-face {
          font-family: "PoppinsMedium";
          src: local('Poppins'), url(${PoppinsMediumTTF}) format('ttf');
          font-display: swap;
          font-weight: 500
        }
        @font-face {
          font-family: "PoppinsSemiBold";
          src: local('Poppins'), url(${PoppinsSemiBoldTTF}) format('ttf');
          font-display: swap;
          font-weight: 600
        }
        @font-face {
          font-family: "PoppinsBold";
          src: local('Poppins'), url(${PoppinsBoldTTF}) format('ttf');
          font-display: swap;
          font-weight: 700
        }
      `,
    },
    MuiAppBar: {
      defaultProps: {
        sx: {
          boxShadow: "10px 14px 28px rgb(35 11 73 / 5%)",
          backgroundColor: (theme) => theme.palette.background.paper,
        },
      },
    },
    MuiButton: {
      defaultProps: {
        size: "large",
        sx: {
          textTransform: "none",
          fontWeight: 500,
        },
      },
      variants: [
        {
          props: { variant: "contained" },
          style: ({ theme }) => ({
            backgroundColor: theme.palette.primary.main,
          }),
        },
        {
          props: { variant: "outlined" },
          style: ({ theme }) => ({
            borderColor: theme.palette.secondary.main,
            borderWidth: 1.25,
            color: theme.palette.secondary.main,
          }),
        },
      ],
    },
    MuiPaper: {
      defaultProps: {
        sx: {
          background: (theme) => theme.palette.background.paper,
        },
      },
    },
    MuiTableCell: {
      defaultProps: {
        sx: {
          borderBottom: (theme) => `1px solid ${theme.palette.divider}`,
        },
      },
    },
    MuiDivider: {
      defaultProps: {
        sx: {
          borderBottom: (theme) => `1px solid ${theme.palette.divider}`,
        },
      },
    },
    MuiFormControlLabel: {
      defaultProps: {
        sx: {
          "& .MuiTypography-root": {
            fontSize: "0.875rem",
            lineHeight: "1.8rem",
            color: (theme) => theme.palette.text.disabled,
          },
        },
      },
    },
    MuiCircularProgress: {
      defaultProps: {
        thickness: 4,
        sx: {
          color: (theme) => theme.palette.primary.main,
        },
      },
    },
    MuiCard: {
      defaultProps: {
        elevation: 0,
        sx: {
          background: (theme) =>
            theme.palette.mode === "light" ? theme.palette.background.paper : theme.palette.divider,
          boxShadow: (theme) => (theme.palette.mode === "light" ? "5px 10px 25px rgba(0, 0, 0, 0.07)" : "none"),
        },
      },
    },
    MuiDialogTitle: {
      defaultProps: {
        sx: {
          fontSize: "1.2rem",
          fontWeight: 600,
          lineHeight: "2.3rem",
        },
      },
    },
    MuiDialog: {},
    MuiMenuItem: {
      defaultProps: {
        sx: {
          marginRight: 1,
          marginLeft: 1,
          borderRadius: 1,
          padding: "12px 8px",
        },
      },
    },
    MuiTypography: {
      defaultProps: {
        variantMapping: {
          label: "span",
        },
      },
    },
  },
};
const light: ThemeOptions = {
  palette: {
    mode: "light",
    primary: {
      main: tariPurple[500],
      dark: tariPurple[800],
      light: tariPurple[400],
    },
    secondary: {
      main: tariPurple[600],
      dark: gothic[500],
      light: teal[400],
    },
    divider: "rgba(0,0,0,0.08)",
    text: {
      primary: grey[950],
      secondary: grey[600],
      disabled: grey[400],
    },
    background: {
      default: tariBg[100],
      paper: tariBg[50],
    },
    success: {
      main: green[500],
      dark: green[600],
      light: green[400],
      contrastText: "#ffffff",
    },
    warning: {
      main: orange[300],
      dark: orange[400],
      light: orange[200],
      contrastText: "#ffffff",
    },
    error: {
      main: red[500],
      dark: red[600],
      light: red[400],
      contrastText: "#ffffff",
    },
    info: {
      main: blue[500],
      dark: blue[700],
      light: blue[400],
      contrastText: "#ffffff",
    },
    accent: {
      background: "rgba(243,243,252,0.2)",
      border: "rgba(0, 0, 0, 0.03)",
    },
  },
};
const dark: ThemeOptions = {
  palette: {
    mode: "dark",
    primary: {
      main: tariPurple[400],
      dark: tariPurple[200],
      light: tariPurple[50],
    },
    secondary: {
      main: tariPurple[300],
      dark: teal[300],
      light: gothic[400],
    },
    divider: "rgba(255,255,255,0.04)",
    text: {
      primary: "#ffffff",
      secondary: grey[100],
      disabled: "rgba(255,255,255,0.4)",
    },
    background: {
      default: tariBg[950],
      paper: tariBg[900],
    },
    success: {
      main: green[500],
      dark: green[400],
      light: green[600],
      contrastText: "#ffffff",
    },
    warning: {
      main: orange[300],
      dark: orange[200],
      light: orange[400],
      contrastText: "#ffffff",
    },
    error: {
      main: red[500],
      dark: red[600],
      light: red[500],
      contrastText: "#ffffff",
    },
    info: {
      main: blue[500],
      dark: blue[700],
      light: blue[400],
      contrastText: "#ffffff",
    },
    accent: {
      background: "rgba(5,4,14,0.3)",
      border: "rgba(255, 255, 255, 0.03)",
    },
  },
};

export { componentSettings, dark, light };
