//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
/// <reference types="vite/client" />

import "./augmentations.d.ts";
import "./theme.css";

import type { Palette, PaletteOptions, TypographyVariants, TypographyVariantsOptions } from "@mui/material/styles";
import type { TypographyPropsVariantOverrides } from "@mui/material/Typography";

import { blue, gothic, green, grey, orange, red, tariBg, tariPurple, teal } from "./theme/colors";
import { componentSettings, dark, light } from "./theme/tokens";

export { blue, componentSettings, dark, gothic, green, grey, light, orange, red, tariBg, tariPurple, teal };
export type { Palette, PaletteOptions, TypographyPropsVariantOverrides, TypographyVariants, TypographyVariantsOptions };
