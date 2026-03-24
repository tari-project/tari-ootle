//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import TransactionReceiptsList from "./components/TransactionReceiptsList";

function TransactionReceiptsLayout() {
  return (
    <>
      <Grid size={12}>
        <PageHeading>Transaction Receipts</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <TransactionReceiptsList />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default TransactionReceiptsLayout;
