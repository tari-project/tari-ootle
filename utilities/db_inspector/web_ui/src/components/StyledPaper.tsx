//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { styled } from "@mui/material/styles";
import { Paper } from "@mui/material";

const StyledPaper = styled(Paper)(({ theme }) => ({
  padding: theme.spacing(3),
  boxShadow: "10px 14px 28px rgba(35, 11, 73, 0.05)",
  border: "1px solid rgba(255,255,255,0.04)",
}));

export default StyledPaper;

