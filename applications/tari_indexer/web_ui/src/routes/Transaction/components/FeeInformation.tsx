import Table from '@mui/material/Table';
import TableBody from '@mui/material/TableBody';
import TableCell from '@mui/material/TableCell';
import TableContainer from '@mui/material/TableContainer';
import TableRow from '@mui/material/TableRow';
import Typography from '@mui/material/Typography';
import Chip from '@mui/material/Chip';
import {
  Accordion,
  AccordionSummary,
  AccordionDetails,
} from '../../../Components/Accordion';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import { DataTableCell } from '../../../Components/StyledComponents';
import { Stack } from '@mui/material';
import { useState, useEffect } from 'react';
import type { FeeReceipt } from '@tari-project/typescript-bindings';
import { formatXTM } from '../../../utils/helpers';

interface FeeInformationProps extends FeeReceipt {
  expandAllTrigger?: number;
  collapseAllTrigger?: number;
  onExpandedChange?: (expanded: boolean) => void;
}

function FeeInformation({
  total_fee_payment,
  total_fees_paid,
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
                <DataTableCell>{formatXTM(total_fees_paid)}</DataTableCell>
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
                      )
                    )}
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
