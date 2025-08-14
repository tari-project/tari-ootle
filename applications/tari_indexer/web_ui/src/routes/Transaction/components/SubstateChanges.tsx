import Table from '@mui/material/Table';
import TableBody from '@mui/material/TableBody';
import TableCell from '@mui/material/TableCell';
import TableContainer from '@mui/material/TableContainer';
import TableRow from '@mui/material/TableRow';
import Typography from '@mui/material/Typography';
import Stack from '@mui/material/Stack';
import {
  Accordion,
  AccordionSummary,
  AccordionDetails,
} from '../../../Components/Accordion';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import Chip from '@mui/material/Chip';
import { DataTableCell } from '../../../Components/StyledComponents';
import { useState, useEffect } from 'react';
import ArrowCircleUpRoundedIcon from '@mui/icons-material/ArrowCircleUpRounded';
import ArrowCircleDownRoundedIcon from '@mui/icons-material/ArrowCircleDownRounded';
import { useTheme } from '@mui/material/styles';

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
  const theme = useTheme();

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
                  <TableCell style={{ verticalAlign: 'top' }}>
                    Down Substates
                  </TableCell>
                  <DataTableCell>
                    <Stack
                      spacing={1}
                      direction="column"
                      alignItems="flex-start"
                    >
                      {result.Accept.down_substates.map(
                        (substate: any, index: number) => (
                          <Stack
                            key={index}
                            direction="row"
                            spacing={1}
                            alignItems="center"
                          >
                            <ArrowCircleDownRoundedIcon
                              sx={{
                                color: theme.palette.warning.main,
                                fontSize: 24,
                              }}
                              fontSize="small"
                            />
                            <Chip label={substate[0]} variant="filled" />
                            <Typography variant="inherit">
                              v{substate[1]}
                            </Typography>
                          </Stack>
                        )
                      )}
                    </Stack>
                  </DataTableCell>
                </TableRow>
              )}
              {result.Accept.up_substates && (
                <TableRow>
                  <TableCell style={{ verticalAlign: 'top' }}>
                    Up Substates
                  </TableCell>
                  <DataTableCell>
                    <Stack
                      spacing={1}
                      direction="column"
                      alignItems="flex-start"
                      justifyContent="flex-start"
                    >
                      {result.Accept.up_substates.map(
                        (substate: any, index: number) => (
                          <Stack
                            key={index}
                            direction="row"
                            spacing={1}
                            alignItems="center"
                          >
                            <ArrowCircleUpRoundedIcon
                              sx={{
                                color: theme.palette.success.main,
                                fontSize: 24,
                              }}
                              fontSize="small"
                            />
                            <Chip label={substate[0]} variant="filled" />
                            <Typography variant="inherit">
                              v{substate[1].version}
                            </Typography>
                          </Stack>
                        )
                      )}
                    </Stack>
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
