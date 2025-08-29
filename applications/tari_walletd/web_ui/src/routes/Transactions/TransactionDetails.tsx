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

import { useState } from "react";
import { useParams } from "react-router-dom";
import { useTransactionDetails } from "../../api/hooks/useTransactions";
import { Accordion, AccordionDetails, AccordionSummary } from "../../Components/Accordion";
import { Grid, Table, TableContainer, TableBody, TableRow, TableCell, Button, Fade, Stack } from "@mui/material";
import Typography from "@mui/material/Typography";
import { saveAs } from "file-saver";
import { DataTableCell, StyledPaper } from "../../Components/StyledComponents";
import PageHeading from "../../Components/PageHeading";
import Events from "./Events";
import Logs from "./Logs";
import Instructions from "./Instructions";
import Substates from "./Substates";
import Inputs from "./Inputs";
import Signers from "./Signers";
import ExecutionResults from "./ExecutionResults";
import FeeReceipt from "./FeeReceipt";
import StatusChip from "../../Components/StatusChip";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import Loading from "../../Components/Loading";
import Error from "../../Components/Error";
import { FinalizeResult, TransactionResult, TransactionSignature } from "@tari-project/typescript-bindings";
import { getRejectReasonFromTransactionResult, rejectReasonToString } from "@tari-project/typescript-bindings";
import { BsQuestionCircle } from "react-icons/bs";

export default function TransactionDetails() {
  const [expandedPanels, setExpandedPanels] = useState<string[]>([]);
  const params = useParams();
  const transactionId = params.id!;
  const { data, isLoading, isError, error } = useTransactionDetails(transactionId);

  const handleChange = (panel: string) => (_event: React.SyntheticEvent, isExpanded: boolean) => {
    setExpandedPanels((prevExpandedPanels) => {
      if (isExpanded) {
        return [...prevExpandedPanels, panel];
      } else {
        return prevExpandedPanels.filter((p) => p !== panel);
      }
    });
  };

  const expandAll = () => {
    setExpandedPanels(["panel1", "panel2", "panel3", "panel4", "panel5", "panel6", "panel7", "panel8", "panel9"]);
  };

  const collapseAll = () => {
    setExpandedPanels([]);
  };

  const renderResult = (result: FinalizeResult | null) => {
    if (result) {
      if ("Accept" in result.result) {
        return <span>Accepted</span>;
      }
      return <span>{rejectReasonToString(getRejectReasonFromTransactionResult(result.result))}</span>;
    } else {
      return <span>In progress</span>;
    }
  };

  const Empty = ({ message }: { message: string }) => (
    <Stack alignItems="center" sx={{ p: 3 }}>
      <Typography variant="body2" color="text.secondary">
        {message}
      </Typography>
    </Stack>
  );

  const renderContent = () => {
    if (isLoading) {
      return <Loading />;
    }

    if (isError) {
      return <Error message={error.message} />;
    }

    if (!data) {
      return null;
    }

    const last_update_time = new Date(data.last_update_time);
    const handleDownload = () => {
      const json = JSON.stringify(data, null, 2);
      const blob = new Blob([json], { type: "application/json" });
      const filename = `tx-${transactionId}.json` || "tx-unknown_id.json";
      saveAs(blob, filename);
    };

    const getTransactionFailure = (txResult: TransactionResult | undefined): string => {
      if (txResult === undefined || "Accept" in txResult) {
        return "No reason";
      }
      let reason;
      if ("AcceptFeeRejectRest" in txResult) {
        reason = txResult.AcceptFeeRejectRest[1];
      } else {
        reason = txResult.Reject;
      }
      if (typeof reason === "string") {
        return reason;
      } else {
        return JSON.stringify(reason);
      }
    };

    const seal_signature = data.transaction.V1?.seal_signature;
    const transaction_body = data.transaction.V1?.body;
    const transaction = transaction_body?.transaction;
    const feeReceipt = data.result?.fee_receipt;

    return (
      <Fade in={!isLoading}>
        <div>
          <>
            <TableContainer>
              <Table>
                <TableBody>
                  <TableRow>
                    <TableCell>Transaction Hash</TableCell>
                    <DataTableCell>{transactionId}</DataTableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell>Timestamp</TableCell>
                    <DataTableCell>{last_update_time.toLocaleString()}</DataTableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell>Total Fees</TableCell>
                    <DataTableCell>
                      {feeReceipt?.total_fees_paid.toString() || 0}
                      {feeReceipt?.total_fee_overcharge ? (
                        <>
                          {" "}
                          ({feeReceipt.total_fee_overcharge} overcharge{" "}
                          <BsQuestionCircle
                            style={{ display: "inline" }}
                            title="An overcharge occurs when paying more fees than required using stealth transfers. To preserve privacy, there is no vault to refund excess fees, therefore the fees are given to validators in their entirety."
                          />
                          )
                        </>
                      ) : (
                        ""
                      )}
                    </DataTableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell>Status</TableCell>
                    <DataTableCell>
                      <StatusChip status={data.status} />
                    </DataTableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell>Result</TableCell>
                    <DataTableCell>
                      {data.invalid_reason ? data.invalid_reason : renderResult(data?.result)}
                    </DataTableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell>JSON</TableCell>
                    <DataTableCell>
                      <Button variant="outlined" onClick={handleDownload}>
                        Download
                      </Button>
                    </DataTableCell>
                  </TableRow>
                  <TableRow>
                    {data?.result?.result ? (
                      <>
                        <TableCell>Reason</TableCell>
                        <DataTableCell>{getTransactionFailure(data?.result?.result)}</DataTableCell>
                      </>
                    ) : (
                      <TableCell>No result yet...</TableCell>
                    )}
                  </TableRow>
                </TableBody>
              </Table>
            </TableContainer>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                padding: "2rem 1rem 0.5rem 1rem",
              }}
            >
              <Typography variant="h5">More Info</Typography>
              <div
                style={{
                  display: "flex",
                  justifyContent: "flex-end",
                  gap: "1rem",
                }}
              >
                <Button
                  onClick={expandAll}
                  style={{
                    fontSize: "0.85rem",
                  }}
                  startIcon={<KeyboardArrowDownIcon />}
                >
                  Expand All
                </Button>
                <Button
                  onClick={collapseAll}
                  style={{
                    fontSize: "0.85rem",
                  }}
                  startIcon={<KeyboardArrowUpIcon />}
                  disabled={expandedPanels.length === 0}
                >
                  Collapse All
                </Button>
              </div>
            </div>
          </>
          <Accordion expanded={expandedPanels.includes("panel1")} onChange={handleChange("panel1")}>
            <AccordionSummary aria-controls="panel1bh-content" id="panel1bh-header">
              <Typography variant="h5">Fee Instructions</Typography>
            </AccordionSummary>
            <AccordionDetails>
              {transaction?.fee_instructions?.length ? (
                <Instructions data={transaction?.fee_instructions} />
              ) : (
                <Empty message="No fee instructions available" />
              )}
            </AccordionDetails>
          </Accordion>
          <Accordion expanded={expandedPanels.includes("panel2")} onChange={handleChange("panel2")}>
            <AccordionSummary aria-controls="panel2bh-content" id="panel1bh-header">
              <Typography variant="h5">Instructions</Typography>
            </AccordionSummary>
            <AccordionDetails>
              {transaction?.instructions?.length ? (
                <Instructions data={transaction.instructions} />
              ) : (
                <Empty message="No instructions available" />
              )}
            </AccordionDetails>
          </Accordion>
          {data.result && (
            <Accordion expanded={expandedPanels.includes("panel3")} onChange={handleChange("panel3")}>
              <AccordionSummary aria-controls="panel3bh-content" id="panel1bh-header">
                <Typography variant="h5">Events</Typography>
              </AccordionSummary>
              <AccordionDetails>
                <Events data={data.result.events} />
              </AccordionDetails>
            </Accordion>
          )}
          {data.result && (
            <Accordion expanded={expandedPanels.includes("panel4")} onChange={handleChange("panel4")}>
              <AccordionSummary aria-controls="panel4bh-content" id="panel1bh-header">
                <Typography variant="h5">Logs</Typography>
              </AccordionSummary>
              <AccordionDetails>
                {data.result.logs?.length ? <Logs data={data.result.logs} /> : <Empty message="No logs available" />}
              </AccordionDetails>
            </Accordion>
          )}
          {data.result && (
            <Accordion expanded={expandedPanels.includes("panel5")} onChange={handleChange("panel5")}>
              <AccordionSummary aria-controls="panel5bh-content" id="panel1bh-header">
                <Typography variant="h5">Substates</Typography>
              </AccordionSummary>
              <AccordionDetails>
                <Substates data={data.result.result} />
              </AccordionDetails>
            </Accordion>
          )}
          {data.result && data.result.execution_results && (
            <Accordion expanded={expandedPanels.includes("panel6")} onChange={handleChange("panel6")}>
              <AccordionSummary aria-controls="panel6bh-content" id="panel6bh-header">
                <Typography variant="h5">Execution Results</Typography>
              </AccordionSummary>
              <AccordionDetails>
                <ExecutionResults data={data.result.execution_results} />
              </AccordionDetails>
            </Accordion>
          )}
          {data.result && data.result.fee_receipt && (
            <Accordion expanded={expandedPanels.includes("panel7")} onChange={handleChange("panel7")}>
              <AccordionSummary aria-controls="panel7bh-content" id="panel7bh-header">
                <Typography variant="h5">Fee Receipt</Typography>
              </AccordionSummary>
              <AccordionDetails>
                <FeeReceipt data={data.result.fee_receipt} />
              </AccordionDetails>
            </Accordion>
          )}
          <Accordion expanded={expandedPanels.includes("panel8")} onChange={handleChange("panel8")}>
            <AccordionSummary aria-controls="panel8bh-content" id="panel8bh-header">
              <Typography variant="h5">Inputs</Typography>
            </AccordionSummary>
            <AccordionDetails>
              <Inputs data={transaction?.inputs || []} />
            </AccordionDetails>
          </Accordion>
          <Accordion expanded={expandedPanels.includes("panel9")} onChange={handleChange("panel9")}>
            <AccordionSummary aria-controls="panel9bh-content" id="panel9bh-header">
              <Typography variant="h5">Signers</Typography>
            </AccordionSummary>
            <AccordionDetails>
              <Signers seal_signature={seal_signature} transaction_body={transaction_body} />
            </AccordionDetails>
          </Accordion>
        </div>
      </Fade>
    );
  };

  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        <PageHeading>Transaction Details</PageHeading>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>{renderContent()}</StyledPaper>
      </Grid>
    </>
  );
}
