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
import Box from '@mui/material/Box';
import Chip from '@mui/material/Chip';
import { DataTableCell } from '../../../Components/StyledComponents';
import { useState, useEffect } from 'react';
import type { Event } from '@tari-project/typescript-bindings';

interface EventsProps {
  events: Event[];
  expandAllTrigger?: number;
  collapseAllTrigger?: number;
  onExpandedChange?: (expanded: boolean) => void;
}

function Events({
  events,
  expandAllTrigger = 0,
  collapseAllTrigger = 0,
  onExpandedChange,
}: EventsProps) {
  const [expanded, setExpanded] = useState(false);

  if (!events || events.length === 0) {
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
        <Typography variant="h6">Events ({events.length})</Typography>
      </AccordionSummary>
      <AccordionDetails>
        <Box>
          {events.map((event: Event, index: number) => (
            <Accordion key={index}>
              <AccordionSummary expandIcon={<ExpandMoreIcon />}>
                <Stack direction="row" spacing={1} alignItems="center">
                  <Chip variant="filled" label={event.topic} color="default" />
                  <Typography variant="subtitle2">
                    {event.substate_id ? String(event.substate_id) : 'N/A'}
                  </Typography>
                </Stack>
              </AccordionSummary>
              <AccordionDetails>
                <TableContainer>
                  <Table size="small">
                    <TableBody>
                      <TableRow>
                        <TableCell>Topic</TableCell>
                        <DataTableCell>{event.topic}</DataTableCell>
                      </TableRow>
                      <TableRow>
                        <TableCell>Substate ID</TableCell>
                        <DataTableCell>
                          {event.substate_id
                            ? String(event.substate_id)
                            : 'N/A'}
                        </DataTableCell>
                      </TableRow>
                      <TableRow>
                        <TableCell>Template Address</TableCell>
                        <DataTableCell>{event.template_address}</DataTableCell>
                      </TableRow>
                      <TableRow>
                        <TableCell>Transaction Hash</TableCell>
                        <DataTableCell>{event.tx_hash}</DataTableCell>
                      </TableRow>
                      <TableRow>
                        <TableCell>Payload</TableCell>
                        <DataTableCell>
                          <pre
                            style={{
                              fontSize: '12px',
                              margin: 0,
                              whiteSpace: 'pre-wrap',
                            }}
                          >
                            {JSON.stringify(event.payload, null, 2)}
                          </pre>
                        </DataTableCell>
                      </TableRow>
                    </TableBody>
                  </Table>
                </TableContainer>
              </AccordionDetails>
            </Accordion>
          ))}
        </Box>
      </AccordionDetails>
    </Accordion>
  );
}

export default Events;
