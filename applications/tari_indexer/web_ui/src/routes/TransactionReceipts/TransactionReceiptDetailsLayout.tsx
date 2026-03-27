//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import { useParams } from "react-router-dom";
import TransactionReceiptDetails from "./components/TransactionReceiptDetails";

function TransactionReceiptDetailsLayout() {
  const { receipt_address } = useParams();
  return (
    <>
      <Grid size={12}>
        <PageHeading>Transaction Receipt</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <TransactionReceiptDetails address={receipt_address || ""} />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default TransactionReceiptDetailsLayout;
