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

import { formatCurrency } from "@/utils/helpers";
import { useTransactionDetails } from "@api/hooks/useTransactions";
import { Accordion, AccordionDetails, AccordionSummary } from "@components/Accordion";
import Error from "@components/Error";
import Loading from "@components/Loading";
import PageHeading from "@components/PageHeading";
import { DataTableCell, StyledPaper } from "@components/StyledComponents";
import TransactionsStatusChip from "@components/TransactionsStatusChip";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
  Button,
  Fade,
  Grid,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
  Tooltip,
} from "@mui/material";
import Typography from "@mui/material/Typography";
import {
  FinalizeResult,
  getRejectReasonFromTransactionResult,
  rejectReasonToString,
  TransactionResult,
} from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";
import { saveAs } from "file-saver";
import { useState } from "react";
import { BsQuestionCircle } from "react-icons/bs";
import { useParams } from "react-router";
import Events from "./Events";
import ExecutionResults from "./ExecutionResults";
import FeeReceipt from "./FeeReceipt";
import Inputs from "./Inputs";
import Instructions from "./Instructions";
import Logs from "./Logs";
import Signers from "./Signers";
import Substates from "./Substates";

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
    setExpandedPanels([
      "panel1",
      "panel2",
      "panel3",
      "panel4",
      "panel5",
      "panel6",
      "panel7",
      "panel8",
      "panel9",
      "panel10",
    ]);
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
      return rejectReasonToString(reason);
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
                      {data.final_fee != null
                        ? formatCurrency(data.final_fee, XTR_CURRENCY)
                        : feeReceipt
                          ? formatCurrency(feeReceipt.total_fees_paid, XTR_CURRENCY)
                          : "0"}
                      {feeReceipt?.total_fee_overcharge ? (
                        <>
                          {" "}
                          ({formatCurrency(feeReceipt.total_fee_overcharge, XTR_CURRENCY)} overcharge{" "}
                          <Tooltip title="An overcharge occurs when paying more fees than required using stealth transfers. To preserve privacy, there is no vault to refund excess fees, therefore the fees are given to validators in their entirety.">
                            <BsQuestionCircle style={{ display: "inline" }} />
                          </Tooltip>
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
                      <TransactionsStatusChip status={data.status} />
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
                <FeeReceipt data={data.result.fee_receipt} finalFee={data.final_fee} />
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
          <Accordion expanded={expandedPanels.includes("panel10")} onChange={handleChange("panel10")}>
            <AccordionSummary aria-controls="panel10bh-content" id="panel10bh-header">
              <Typography variant="h5">Blobs ({transaction?.blob_hashes?.length ?? 0})</Typography>
            </AccordionSummary>
            <AccordionDetails>
              <Blobs hashes={transaction?.blob_hashes || []} sizes={transaction?.blob_sizes || []} />
            </AccordionDetails>
          </Accordion>
        </div>
      </Fade>
    );
  };

  return (
    <>
      <Grid size={12}>
        <PageHeading>Transaction Details</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>{renderContent()}</StyledPaper>
      </Grid>
    </>
  );
}

function formatBlobSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MiB`;
}

/**
 * Renders the blob commitments carried by a (pruned) transaction. Only blob hashes and sizes
 * are available — raw payloads are omitted from the API response.
 */
function Blobs({ hashes, sizes }: { hashes: string[]; sizes: number[] }) {
  if (hashes.length === 0) {
    return (
      <Stack alignItems="center" sx={{ p: 3 }}>
        <Typography variant="body2" color="text.secondary">
          This transaction has no blobs.
        </Typography>
      </Stack>
    );
  }
  return (
    <Table size="small">
      <TableBody>
        <TableRow>
          <TableCell sx={{ fontWeight: 600 }}>Index</TableCell>
          <TableCell sx={{ fontWeight: 600 }}>Hash</TableCell>
          <TableCell sx={{ fontWeight: 600 }}>Size</TableCell>
        </TableRow>
        {hashes.map((hash, i) => {
          const size = sizes[i];
          return (
            <TableRow key={`${i}-${hash}`}>
              <DataTableCell sx={{ width: "10%" }}>{i}</DataTableCell>
              <DataTableCell sx={{ fontFamily: "monospace", fontSize: "0.8rem", wordBreak: "break-all" }}>
                {hash}
              </DataTableCell>
              <DataTableCell sx={{ width: "15%" }}>{size != null ? formatBlobSize(size) : "—"}</DataTableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
