import Typography from '@mui/material/Typography';
import {
  Accordion,
  AccordionSummary,
  AccordionDetails,
} from '../../../Components/Accordion';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import Chip from '@mui/material/Chip';
import { Stack } from '@mui/material';
import { useState, useEffect } from 'react';

interface Log {
  level: string;
  message: string;
}

interface LogsProps {
  logs: Log[];
  expandAllTrigger?: number;
  collapseAllTrigger?: number;
  onExpandedChange?: (expanded: boolean) => void;
}

function Logs({
  logs,
  expandAllTrigger = 0,
  collapseAllTrigger = 0,
  onExpandedChange,
}: LogsProps) {
  const [expanded, setExpanded] = useState(false);

  if (!logs || logs.length === 0) {
    return null;
  }

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
        <Typography variant="h6">Logs ({logs.length})</Typography>
      </AccordionSummary>
      <AccordionDetails>
        <Stack spacing={1}>
          {logs.map((log: Log, index: number) => (
            <Stack key={index} direction="row" spacing={1} alignItems="center">
              <Chip
                label={log.level}
                variant="filled"
                color={
                  log.level === 'Debug'
                    ? 'default'
                    : log.level === 'Info'
                    ? 'info'
                    : 'error'
                }
              />
              <Typography variant="body2">{log.message}</Typography>
            </Stack>
          ))}
        </Stack>
      </AccordionDetails>
    </Accordion>
  );
}

export default Logs;
