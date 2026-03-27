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
import Typography from "@mui/material/Typography";
import Chip from "@mui/material/Chip";
import {
  Accordion,
  AccordionSummary,
  AccordionDetails,
} from "../../../Components/Accordion";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import { DataTableCell } from "../../../Components/StyledComponents";
import { Stack } from "@mui/material";
import { useState, useEffect } from "react";
import type { FeeReceipt } from "@tari-project/ootle-ts-bindings";
import { formatXTM } from "../../../utils/helpers";

interface FeeInformationProps extends FeeReceipt {
  expandAllTrigger?: number;
  collapseAllTrigger?: number;
  onExpandedChange?: (expanded: boolean) => void;
}

function FeeInformation({
                          total_fee_payment,
                          total_fees_paid,
                          total_fee_overcharge,
                          cost_breakdown,
                          expandAllTrigger = 0,
                          collapseAllTrigger = 0,
                          onExpandedChange,
                        }: FeeInformationProps) {
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (expandAllTrigger > 0) {
      setExpanded(true);
    }
  }, [expandAllTrigger]);

  useEffect(() => {
    if (collapseAllTrigger > 0) {
      setExpanded(false);
    }
  }, [collapseAllTrigger]);

  useEffect(() => {
    onExpandedChange?.(expanded);
  }, [expanded, onExpandedChange]);

  const handleChange = (event: React.SyntheticEvent, isExpanded: boolean) => {
    event.stopPropagation();
    setExpanded(isExpanded);
  };

  const totalCost = Object.entries(cost_breakdown.breakdown).reduce((sum, [_, value]) => BigInt(sum) + BigInt(value), BigInt(0));

  return (
    <Accordion expanded={expanded} onChange={handleChange}>
      <AccordionSummary expandIcon={<ExpandMoreIcon />}>
        <Typography variant="h6">Fee Information</Typography>
      </AccordionSummary>
      <AccordionDetails>
        <TableContainer>
          <Table>
            <TableBody>
              <TableRow>
                <TableCell>Total Fee Payment</TableCell>
                <DataTableCell>{formatXTM(total_fee_payment)}</DataTableCell>
              </TableRow>
              <TableRow>
                <TableCell>Total Fees Paid</TableCell>
                <DataTableCell>{formatXTM(total_fees_paid)}{total_fee_overcharge > 0 ? ` Overcharge: ${total_fee_overcharge}` : ""}</DataTableCell>
              </TableRow>
              <TableRow>
                <TableCell>Cost Breakdown</TableCell>
                <DataTableCell>
                  <Stack direction="row" spacing={1}>
                    {Object.entries(cost_breakdown.breakdown).map(
                      ([key, value]) => (
                        <Chip
                          key={key}
                          label={`${key}: ${value}`}
                          variant="filled"
                          color="default"
                        />
                      ),
                    )}
                    <Chip
                      label={`Total: ${totalCost}`}
                      variant="outlined"
                      color="primary"
                    />
                  </Stack>
                </DataTableCell>
              </TableRow>
            </TableBody>
          </Table>
        </TableContainer>
      </AccordionDetails>
    </Accordion>
  );
}

export default FeeInformation;
