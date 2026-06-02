//  Copyright 2025. The Tari Project
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

import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
  Alert,
  Box,
  Button,
  Chip,
  Fade,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
  Typography,
} from "@mui/material";
import type {
  IndexerGetTransactionResultRequest,
  ListRecentTransactionsResponse,
} from "@tari-project/ootle-ts-bindings";
import { useQueryClient } from "@tanstack/react-query";
import { saveAs } from "file-saver";
import { useState } from "react";
import { useGetTransaction, useGetTransactionResult } from "../../../api/hooks/useTransactions";
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
} from "../../../Components/Accordion";
import FetchStatusCheck from "../../../Components/FetchStatusCheck";
import StatusChip from "../../../Components/StatusChip";
import { DataTableCell } from "../../../Components/StyledComponents";
import { CURRENCY } from "../../../utils/constants";
import { formatCurrency, validateHash } from "../../../utils/helpers";
import EventsContent from "./EventsContent";
import ExecutionResults from "./ExecutionResults";
import FeeReceipt from "./FeeReceipt";
import Inputs from "./Inputs";
import Instructions from "./Instructions";
import LogsContent from "./LogsContent";
import Signers from "./Signers";
import SubstatesContent from "./SubstatesContent";

const isFinalized = (result: any): result is { Finalized: any } =>
  typeof result === "object" && result !== null && "Finalized" in result;

const isAcceptResult = (result: any): result is { Accept: any } =>
  result && typeof result === "object" && "Accept" in result;

const Empty = ({ message }: { message: string }) => (
  <Stack alignItems="center" sx={{ p: 3 }}>
    <Typography variant="body2" color="text.secondary">
      {message}
    </Typography>
  </Stack>
);

function Result({ transaction_id }: IndexerGetTransactionResultRequest) {
  const [expandedPanels, setExpandedPanels] = useState<string[]>([]);
  const normalizedId = transaction_id.toLowerCase();
  const isValidHash = validateHash(normalizedId);
  const { data, isLoading, error, isError } = useGetTransactionResult(normalizedId);

  const queryClient = useQueryClient();
  const cachedList = queryClient.getQueryData<ListRecentTransactionsResponse>(["recent_transactions"]);
  const cachedEntry = cachedList?.transactions?.find((tx) => tx.transaction_id === normalizedId);

  // The recent-transactions list cache is only populated when arriving from the list page. On a fresh
  // page load / direct navigation it's empty, so fetch the transaction body directly as a fallback. The
  // result endpoint never carries instructions, hence this separate fetch.
  const { data: fetchedTransaction } = useGetTransaction(normalizedId, isValidHash && !cachedEntry);

  const txEntry = cachedEntry ?? fetchedTransaction?.transaction;
  const txV1 = txEntry?.transaction?.V1;
  const txBody = txV1?.body;
  const transaction = txBody?.transaction;

  if (!isValidHash) {
    return <Alert severity="error">Invalid Hash</Alert>;
  }

  const handleChange = (panel: string) => (_event: React.SyntheticEvent, isExpanded: boolean) => {
    setExpandedPanels((prev) =>
      isExpanded ? [...prev, panel] : prev.filter((p) => p !== panel),
    );
  };

  const expandAll = () =>
    setExpandedPanels(["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9", "p10"]);

  const collapseAll = () => setExpandedPanels([]);

  return (
    <FetchStatusCheck
      isLoading={isLoading}
      isError={isError}
      errorMessage={error ? error.message : "Error fetching transaction details."}
    >
      <Fade in={!isLoading && !isError}>
        <Box>
          {data?.result && isFinalized(data.result) ? (
            <>
              {/* Summary table */}
              <TableContainer sx={{ mb: 2 }}>
                <Table>
                  <TableBody>
                    <TableRow>
                      <TableCell>Transaction Hash</TableCell>
                      <DataTableCell>{normalizedId}</DataTableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell>Decision</TableCell>
                      <DataTableCell>
                        <StatusChip status={data.result.Finalized.final_decision} showTitle={true} />
                      </DataTableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell>Finalized Time</TableCell>
                      <DataTableCell>{data.result.Finalized.finalized_time || "N/A"}</DataTableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell>Execution Time</TableCell>
                      <DataTableCell>
                        {data.result.Finalized.execution_result?.execution_time
                          ? `${data.result.Finalized.execution_result.execution_time.secs}s ${Math.round(
                              data.result.Finalized.execution_result.execution_time.nanos / 1_000_000,
                            )}ms`
                          : "N/A"}
                      </DataTableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell>Total Fees</TableCell>
                      <DataTableCell>
                        {data.result.Finalized.execution_result?.finalize?.fee_receipt?.total_fees_paid
                          ? formatCurrency(
                              data.result.Finalized.execution_result.finalize.fee_receipt.total_fees_paid,
                              CURRENCY.DECIMALS,
                              CURRENCY.SYMBOL,
                            )
                          : "--"}
                      </DataTableCell>
                    </TableRow>
                    {data.result.Finalized.abort_details && (
                      <TableRow>
                        <TableCell>Abort Details</TableCell>
                        <DataTableCell>{data.result.Finalized.abort_details}</DataTableCell>
                      </TableRow>
                    )}
                    <TableRow>
                      <TableCell>Download</TableCell>
                      <DataTableCell>
                        <Button
                          variant="outlined"
                          size="small"
                          onClick={() => {
                            const json = JSON.stringify(
                              { result: data.result, transaction: txEntry?.transaction },
                              null,
                              2,
                            );
                            const blob = new Blob([json], { type: "application/json" });
                            saveAs(blob, `tx-${normalizedId}.json`);
                          }}
                        >
                          Download JSON
                        </Button>
                      </DataTableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </TableContainer>

              {/* Expand / Collapse controls */}
              <Stack direction="row" justifyContent="space-between" alignItems="center" sx={{ px: 1, pb: 1 }}>
                <Typography variant="h5">Details</Typography>
                <Stack direction="row" spacing={1}>
                  <Button size="small" startIcon={<KeyboardArrowDownIcon />} onClick={expandAll}>
                    Expand All
                  </Button>
                  <Button
                    size="small"
                    startIcon={<KeyboardArrowUpIcon />}
                    onClick={collapseAll}
                    disabled={expandedPanels.length === 0}
                  >
                    Collapse All
                  </Button>
                </Stack>
              </Stack>

              {/* Fee Instructions */}
              <Accordion expanded={expandedPanels.includes("p1")} onChange={handleChange("p1")}>
                <AccordionSummary>
                  <Typography variant="h5">Fee Instructions</Typography>
                </AccordionSummary>
                <AccordionDetails>
                  {transaction?.fee_instructions?.length ? (
                    <Instructions data={transaction.fee_instructions} />
                  ) : (
                    <Empty message="No fee instructions available" />
                  )}
                </AccordionDetails>
              </Accordion>

              {/* Instructions */}
              <Accordion expanded={expandedPanels.includes("p2")} onChange={handleChange("p2")}>
                <AccordionSummary>
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

              {/* Blobs */}
              <Accordion expanded={expandedPanels.includes("p10")} onChange={handleChange("p10")}>
                <AccordionSummary>
                  <Typography variant="h5">
                    Blobs ({transaction?.blob_hashes?.length ?? 0})
                  </Typography>
                </AccordionSummary>
                <AccordionDetails>
                  <BlobsContent
                    hashes={transaction?.blob_hashes || []}
                    sizes={transaction?.blob_sizes || []}
                  />
                </AccordionDetails>
              </Accordion>

              {/* Events */}
              {data.result.Finalized.execution_result?.finalize?.events?.length ? (
                <Accordion expanded={expandedPanels.includes("p3")} onChange={handleChange("p3")}>
                  <AccordionSummary>
                    <Typography variant="h5">
                      Events ({data.result.Finalized.execution_result.finalize.events.length})
                    </Typography>
                  </AccordionSummary>
                  <AccordionDetails>
                    <EventsContent data={data.result.Finalized.execution_result.finalize.events} />
                  </AccordionDetails>
                </Accordion>
              ) : null}

              {/* Logs */}
              {data.result.Finalized.execution_result?.finalize?.logs?.length ? (
                <Accordion expanded={expandedPanels.includes("p4")} onChange={handleChange("p4")}>
                  <AccordionSummary>
                    <Typography variant="h5">
                      Logs ({data.result.Finalized.execution_result.finalize.logs.length})
                    </Typography>
                  </AccordionSummary>
                  <AccordionDetails>
                    <LogsContent data={data.result.Finalized.execution_result.finalize.logs} />
                  </AccordionDetails>
                </Accordion>
              ) : null}

              {/* Substates */}
              {data.result.Finalized.execution_result?.finalize?.result &&
                isAcceptResult(data.result.Finalized.execution_result.finalize.result) && (
                  <Accordion expanded={expandedPanels.includes("p5")} onChange={handleChange("p5")}>
                    <AccordionSummary>
                      <Typography variant="h5">Substates</Typography>
                    </AccordionSummary>
                    <AccordionDetails>
                      <SubstatesContent result={data.result.Finalized.execution_result.finalize.result} />
                    </AccordionDetails>
                  </Accordion>
                )}

              {/* Execution Results */}
              {data.result.Finalized.execution_result?.finalize?.execution_results?.length ? (
                <Accordion expanded={expandedPanels.includes("p6")} onChange={handleChange("p6")}>
                  <AccordionSummary>
                    <Typography variant="h5">Execution Results</Typography>
                  </AccordionSummary>
                  <AccordionDetails>
                    <ExecutionResults data={data.result.Finalized.execution_result.finalize.execution_results} />
                  </AccordionDetails>
                </Accordion>
              ) : null}

              {/* Fee Receipt */}
              {data.result.Finalized.execution_result?.finalize?.fee_receipt && (
                <Accordion expanded={expandedPanels.includes("p7")} onChange={handleChange("p7")}>
                  <AccordionSummary>
                    <Typography variant="h5">Fee Receipt</Typography>
                  </AccordionSummary>
                  <AccordionDetails>
                    <FeeReceipt data={data.result.Finalized.execution_result.finalize.fee_receipt} />
                  </AccordionDetails>
                </Accordion>
              )}

              {/* Inputs */}
              <Accordion expanded={expandedPanels.includes("p8")} onChange={handleChange("p8")}>
                <AccordionSummary>
                  <Typography variant="h5">Inputs</Typography>
                </AccordionSummary>
                <AccordionDetails>
                  <Inputs data={transaction?.inputs || []} />
                </AccordionDetails>
              </Accordion>

              {/* Signers */}
              <Accordion expanded={expandedPanels.includes("p9")} onChange={handleChange("p9")}>
                <AccordionSummary>
                  <Typography variant="h5">Signers</Typography>
                </AccordionSummary>
                <AccordionDetails>
                  <Signers seal_signature={txV1?.seal_signature} transaction_body={txBody} />
                </AccordionDetails>
              </Accordion>
            </>
          ) : (
            <TableContainer>
              <Table>
                <TableBody>
                  <TableRow>
                    <TableCell>Transaction Hash</TableCell>
                    <DataTableCell>{normalizedId}</DataTableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell>Status</TableCell>
                    <DataTableCell>
                      <Chip label="Pending" color="warning" variant="filled" />
                    </DataTableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </TableContainer>
          )}
        </Box>
      </Fade>
    </FetchStatusCheck>
  );
}

function formatBlobSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MiB`;
}

function BlobsContent({ hashes, sizes }: { hashes: string[]; sizes: number[] }) {
  if (hashes.length === 0) {
    return <Empty message="This transaction has no blobs." />;
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

export default Result;
