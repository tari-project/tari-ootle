import Table from '@mui/material/Table';
import TableBody from '@mui/material/TableBody';
import TableCell from '@mui/material/TableCell';
import TableContainer from '@mui/material/TableContainer';
import TableRow from '@mui/material/TableRow';
import Typography from '@mui/material/Typography';
import Stack from '@mui/material/Stack';
import Box from '@mui/material/Box';
import {
  Accordion,
  AccordionSummary,
  AccordionDetails,
} from '../../../Components/Accordion';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import Chip from '@mui/material/Chip';
import { DataTableCell } from '../../../Components/StyledComponents';
import { useState, useEffect } from 'react';

interface AcceptResult {
  Accept: {
    down_substates?: any[];
    up_substates?: any[];
  };
}

interface SubstateChangesProps {
  result: AcceptResult;
  expandAllTrigger?: number;
  collapseAllTrigger?: number;
  onExpandedChange?: (expanded: boolean) => void;
}

function SubstateChanges({
  result,
  expandAllTrigger = 0,
  collapseAllTrigger = 0,
  onExpandedChange,
}: SubstateChangesProps) {
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
        <Typography variant="h6">Substate Changes</Typography>
      </AccordionSummary>
      <AccordionDetails>
        <TableContainer>
          <Table>
            <TableBody>
              {result.Accept.down_substates && (
                <TableRow>
                  <TableCell>Down Substates</TableCell>
                  <DataTableCell>
                    <Stack
                      spacing={1}
                      direction="column"
                      alignItems="flex-start"
                    >
                      {result.Accept.down_substates.map(
                        (substate: any, index: number) => (
                          <Box key={index}>
                            <Chip label={substate[0]} variant="filled" /> v
                            {substate[1]}
                          </Box>
                        )
                      )}
                    </Stack>
                  </DataTableCell>
                </TableRow>
              )}
              {result.Accept.up_substates && (
                <TableRow>
                  <TableCell>Up Substates</TableCell>
                  <DataTableCell>
                    <Typography variant="caption">
                      ({result.Accept.up_substates.length} substates
                      created/updated)
                    </Typography>
                  </DataTableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </TableContainer>
      </AccordionDetails>
    </Accordion>
  );
}

export default SubstateChanges;
