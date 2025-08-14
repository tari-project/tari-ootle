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

import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableRow from "@mui/material/TableRow";
import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Alert from "@mui/material/Alert";
import StatusChip from "../../../Components/StatusChip";
import { DataTableCell } from "../../../Components/StyledComponents";
import { useGetTransactionResult } from "../../../api/hooks/useTransactions";
import FetchStatusCheck from "../../../Components/FetchStatusCheck";
import AccordionGroup from "../../../Components/AccordionGroup";
import FeeInformation from "./FeeInformation";
import Events from "./Events";
import Logs from "./Logs";
import SubstateChanges from "./SubstateChanges";
import type {
  IndexerTransactionFinalizedResult,
  IndexerGetTransactionResultRequest,
} from "@tari-project/typescript-bindings";
import { validateHash } from "../../../utils/helpers";

// Type guard to check if result is finalized
const isFinalized = (
  result: IndexerTransactionFinalizedResult
): result is { Finalized: any } => {
  return typeof result === "object" && result !== null && "Finalized" in result;
};

// Type guard to check if transaction result is Accept
const isAcceptResult = (result: any): result is { Accept: any } => {
  return result && typeof result === "object" && "Accept" in result;
};

function Result({ transaction_id }: IndexerGetTransactionResultRequest) {
  const normalizedTransactionId = transaction_id.toLowerCase();
  const isValidHash = validateHash(normalizedTransactionId);
  const { data, isLoading, error, isError } = useGetTransactionResult(
    normalizedTransactionId
  );

  if (!isValidHash) {
    return <Alert severity="error">Invalid Hash</Alert>;
  }

  return (
    <>
      <FetchStatusCheck
        isLoading={isLoading}
        isError={isError}
        errorMessage={
          error ? error.message : "Error fetching transaction details."
        }
      >
        <Box>
          {data?.result && isFinalized(data.result) ? (
            <>
              <TableContainer sx={{ marginBottom: "16px" }}>
                <Table>
                  <TableBody>
                    <TableRow>
                      <TableCell>Transaction Hash</TableCell>
                      <DataTableCell>{normalizedTransactionId}</DataTableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell>Final Decision</TableCell>
                      <DataTableCell>
                        <StatusChip
                          status={data.result.Finalized.final_decision}
                          showTitle={true}
                        />
                      </DataTableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell>Finalized Time</TableCell>
                      <DataTableCell>
                        {data.result.Finalized.finalized_time || "N/A"}
                      </DataTableCell>
                    </TableRow>
                    <TableRow>
                      <TableCell>Execution Time</TableCell>
                      <DataTableCell>
                        {data.result.Finalized.execution_result?.execution_time
                          ? `${
                              data.result.Finalized.execution_result
                                .execution_time.secs
                            }s ${Math.round(
                              data.result.Finalized.execution_result
                                .execution_time.nanos / 1000000
                            )}ms`
                          : "N/A"}
                      </DataTableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </TableContainer>

              <AccordionGroup>
                {data.result.Finalized.execution_result?.finalize
                  ?.fee_receipt && (
                  <FeeInformation
                    {...data.result.Finalized.execution_result.finalize
                      .fee_receipt}
                  />
                )}

                <Events
                  events={
                    data.result.Finalized.execution_result?.finalize?.events ||
                    []
                  }
                />

                <Logs
                  logs={
                    data.result.Finalized.execution_result?.finalize?.logs || []
                  }
                />

                {data.result.Finalized.execution_result?.finalize?.result &&
                  isAcceptResult(
                    data.result.Finalized.execution_result.finalize.result
                  ) && (
                    <SubstateChanges
                      result={
                        data.result.Finalized.execution_result.finalize.result
                      }
                    />
                  )}
              </AccordionGroup>
            </>
          ) : (
            <TableContainer>
              <Table>
                <TableBody>
                  <TableRow>
                    <TableCell>Status</TableCell>
                    <DataTableCell>
                      <Chip
                        label={
                          data?.result === "Pending" ? "Pending" : "Unknown"
                        }
                        color="warning"
                        variant="filled"
                      />
                    </DataTableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell>Transaction Hash</TableCell>
                    <DataTableCell>{normalizedTransactionId}</DataTableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </TableContainer>
          )}
        </Box>
      </FetchStatusCheck>
    </>
  );
}

export default Result;
