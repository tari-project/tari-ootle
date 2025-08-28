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
}
