//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import "@mui/material/styles";
declare module "@mui/material/styles" {
  interface Palette {
    accent: {
      background: string;
      border: string;
    };
  }

  interface PaletteOptions {
    accent?: {
      background?: string;
      border?: string;
    };
  }

  interface TypographyVariants {
    label: React.CSSProperties;
    span: React.CSSProperties;
  }

  // allow configuration using `createTheme()`
  interface TypographyVariantsOptions {
    label?: React.CSSProperties;
    span?: React.CSSProperties;
  }
}

declare module "@mui/material/Typography" {
  interface TypographyPropsVariantOverrides {
    label: true;
    span: true;
  }
}
