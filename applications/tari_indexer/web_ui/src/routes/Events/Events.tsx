//  Copyright 2024. The Tari Project
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

import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import {
  Box,
  Button,
  IconButton,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  TextField,
  Typography,
} from "@mui/material";
import React, { useEffect, useState } from "react";
import { truncateText } from "../../utils/helpers";
import KeyboardArrowLeftIcon from "@mui/icons-material/KeyboardArrowLeft";
import KeyboardArrowRightIcon from "@mui/icons-material/KeyboardArrowRight";
import saveAs from "file-saver";
import JsonDialog from "../../Components/JsonDialog";
import { queryTransactionEvents } from "../../utils/api";
import CopyToClipboard from "../../Components/CopyToClipboard";
import { Link } from "react-router-dom";
import { Event, TransactionId } from "@tari-project/ootle-ts-bindings";

const PAGE_SIZE = 10;

function EventsLayout() {
  const [events, setEvents] = useState<Array<[string, Event]>>([]);
  const [page, setPage] = useState(0);
  const [jsonDialogOpen, setJsonDialogOpen] = React.useState(false);
  const [selectedPayload, setSelectedPayload] = useState({});
  const [filter, setFilter] = useState({
    topic: "",
    substate_id: "",
  });

  useEffect(() => {
    getEvents(page, PAGE_SIZE, filter)
      .then(setEvents)
  }, []);

  async function getEvents(offset: number, limit: number, filter: any) {
    const resp = await queryTransactionEvents({
      topic: filter.topic,
      substate_id: filter.substate_id || null,
      limit,
      offset,
    });


    return resp.events;
  }

  async function handleChangePage(newPage: number) {
    const offset = newPage * PAGE_SIZE;
    const events = await getEvents(offset, PAGE_SIZE, filter);
    setEvents(events);
    setPage(newPage);
  }


  const handlePayloadDownload = (txId: TransactionId, event: Event) => {
    const data = event.payload;
    const json = JSON.stringify(data, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const filename = `event-${txId}-${event.topic}.json`;
    saveAs(blob, filename);
  };

  const handlePayloadView = (event: Event) => {
    setSelectedPayload(event.payload);
    setJsonDialogOpen(true);
  };

  const handleJsonDialogClose = () => {
    setJsonDialogOpen(false);
  };

  const onFilterChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const newFilter = {
      ...filter,
      [e.target.name]: e.target.value,
    };

    setFilter(newFilter);

    const offset = 0;
    let events = await getEvents(offset, PAGE_SIZE, newFilter);
    setEvents(events);
    setPage(0);
  };

  return (
    <>
      <Grid size={12}>
        <PageHeading>Events</PageHeading>
      </Grid>
      <Grid>
        <Box className="flex-container" sx={{ marginBottom: 4 }}>
          <TextField
            name="topic"
            label="Topic"
            value={filter.topic}
            onChange={async (e: React.ChangeEvent<HTMLInputElement>) =>
              onFilterChange(e)
            }
            style={{ flexGrow: 1 }}
          />
          <TextField
            name="substate_id"
            label="Substate Id"
            value={filter.substate_id}
            onChange={async (e: React.ChangeEvent<HTMLInputElement>) =>
              onFilterChange(e)
            }
            style={{ flexGrow: 1 }}
          />
        </Box>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <Table sx={{ minWidth: 650 }} aria-label="simple table">
            <TableHead>
              <TableRow>
                <TableCell>Topic</TableCell>
                <TableCell>Transaction</TableCell>
                <TableCell>Substate Id</TableCell>
                <TableCell>Template</TableCell>
                <TableCell>Payload</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {events.map(([txId, event], i) => (
                <TableRow key={i} sx={{ "&:last-child td, &:last-child th": { border: 0 } }}>
                  <TableCell>{event.topic}</TableCell>
                  <TableCell>
                    {truncateText(txId, 20)}
                    <CopyToClipboard copy={txId} />
                  </TableCell>
                  <TableCell>
                    {event.substate_id ? (
                      <Link to={`/substates?address=${encodeURIComponent(event.substate_id)}`}>
                        {truncateText(event.substate_id, 20)}
                      </Link>
                    ) : (
                      truncateText(event.substate_id, 20)
                    )}
                    <CopyToClipboard copy={event.substate_id || ""} />
                  </TableCell>
                  <TableCell>
                    {event.template_address ? (
                      <Link to={`/templates?address=${encodeURIComponent(event.template_address)}`}>
                        {truncateText(event.template_address, 20)}
                      </Link>
                    ) : (
                      truncateText(event.template_address, 20)
                    )}
                    <CopyToClipboard copy={event.template_address} />
                  </TableCell>
                  <TableCell>
                    <Stack direction="row" spacing={2} alignItems="left">
                      <Button
                        variant="outlined"
                        onClick={() => handlePayloadView(event)}
                      >
                        View
                      </Button>
                      <Button
                        variant="outlined"
                        onClick={() => handlePayloadDownload(txId, event)}
                      >
                        Download
                      </Button>
                    </Stack>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <Stack
            direction="row"
            justifyContent="right"
            spacing={2}
            alignItems="center"
          >
            <IconButton
              aria-label="copy"
              onClick={() => handleChangePage(Math.max(page - 1, 0))}
            >
              <KeyboardArrowLeftIcon />
            </IconButton>
            <Typography sx={{}}>{page}</Typography>
            <IconButton
              aria-label="copy"
              onClick={() => handleChangePage(page + 1)}
            >
              <KeyboardArrowRightIcon />
            </IconButton>
          </Stack>
        </StyledPaper>
      </Grid>
      <JsonDialog
        open={jsonDialogOpen}
        onClose={handleJsonDialogClose}
        data={selectedPayload}
      />
    </>
  );
}

export default EventsLayout;
