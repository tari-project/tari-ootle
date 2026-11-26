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

import KeyboardArrowLeftIcon from "@mui/icons-material/KeyboardArrowLeft";
import KeyboardArrowRightIcon from "@mui/icons-material/KeyboardArrowRight";
import {
  Alert,
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
import Grid from "@mui/material/Grid";
import { Event, TransactionId } from "@tari-project/ootle-ts-bindings";
import saveAs from "file-saver";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import CopyToClipboard from "../../Components/CopyToClipboard";
import JsonDialog from "../../Components/JsonDialog";
import PageHeading from "../../Components/PageHeading";
import { StyledPaper } from "../../Components/StyledComponents";
import { queryTransactionEvents } from "../../utils/api";
import { encodedByteLength, useDecoder } from "../../utils/encoded";
import { truncateText } from "../../utils/helpers";

const PAGE_SIZE = 10;

type Cursor = number | bigint | string;

interface EventFilter {
  topic: string;
  substate_id: string;
}

function EventsLayout() {
  const [encodedEvents, setEncodedEvents] = useState<Array<[string, string]>>([]);
  const { decodeEvent } = useDecoder();
  const events = useMemo(
    () => encodedEvents.map(([txId, encoded]) => ({ txId, encoded, event: decodeEvent(encoded) })),
    [encodedEvents, decodeEvent],
  );
  const [page, setPage] = useState(0);
  // cursors[n] is the before_id that loads page n; page 0 starts from the newest event.
  const [cursors, setCursors] = useState<Array<Cursor | null>>([null]);
  const [nextCursor, setNextCursor] = useState<Cursor | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const requestIdRef = useRef(0);
  const [jsonDialogOpen, setJsonDialogOpen] = React.useState(false);
  const [selectedPayload, setSelectedPayload] = useState({});
  const [filter, setFilter] = useState<EventFilter>({
    topic: "",
    substate_id: "",
  });

  const loadPage = useCallback(
    async (
      beforeId: Cursor | null,
      activeFilter: EventFilter,
      targetPage: number,
      nextCursors?: Array<Cursor | null>,
    ) => {
      const reqId = ++requestIdRef.current;
      setLoading(true);
      setError(null);
      try {
        const resp = await queryTransactionEvents({
          topic: activeFilter.topic.trim() || null,
          substate_id: activeFilter.substate_id.trim() || null,
          limit: PAGE_SIZE,
          offset: null,
          before_id: beforeId,
        });
        if (reqId !== requestIdRef.current) return;
        setEncodedEvents(resp.events);
        setNextCursor(resp.next_before_id ?? null);
        setPage(targetPage);
        if (nextCursors) {
          setCursors(nextCursors);
        }
      } catch (err: unknown) {
        if (reqId !== requestIdRef.current) return;
        setError(err instanceof Error && err.message ? err.message : "Failed to load events.");
      } finally {
        if (reqId === requestIdRef.current) {
          setLoading(false);
        }
      }
    },
    [],
  );

  useEffect(() => {
    void loadPage(null, { topic: "", substate_id: "" }, 0, [null]);
    return () => {
      requestIdRef.current++;
    };
  }, [loadPage]);

  function handleChangePage(newPage: number) {
    if (loading || newPage < 0 || newPage === page) return;
    if (newPage > page) {
      if (nextCursor === null) {
        return;
      }
      const updatedCursors = [...cursors.slice(0, newPage), nextCursor];
      void loadPage(nextCursor, filter, newPage, updatedCursors);
    } else {
      void loadPage(cursors[newPage] ?? null, filter, newPage);
    }
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

  const onFilterChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newFilter: EventFilter = {
      ...filter,
      [e.target.name]: e.target.value,
    };

    setFilter(newFilter);
    setEncodedEvents([]);
    setNextCursor(null);
    setCursors([null]);
    setPage(0);
    void loadPage(null, newFilter, 0, [null]);
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
            onChange={onFilterChange}
            style={{ flexGrow: 1 }}
          />
          <TextField
            name="substate_id"
            label="Substate Id"
            value={filter.substate_id}
            onChange={onFilterChange}
            style={{ flexGrow: 1 }}
          />
        </Box>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          {error && (
            <Alert severity="error" sx={{ marginBottom: 2 }}>
              {error}
            </Alert>
          )}
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
              {events.length === 0 ? (
                loading ? (
                  <TableRow>
                    <TableCell colSpan={5}>
                      <Typography color="textSecondary">Loading...</Typography>
                    </TableCell>
                  </TableRow>
                ) : !error ? (
                  <TableRow>
                    <TableCell colSpan={5}>
                      <Typography color="textSecondary">No events found.</Typography>
                    </TableCell>
                  </TableRow>
                ) : null
              ) : (
                events.map(({ txId, encoded, event }, i) =>
                  event ? (
                    <TableRow key={i} sx={{ "&:last-child td, &:last-child th": { border: 0 } }}>
                      <TableCell>{event.topic}</TableCell>
                      <TableCell>
                        {truncateText(txId, 20)}
                        <CopyToClipboard copy={txId} />
                      </TableCell>
                      <TableCell>
                        {event.substate_id ? (
                          <>
                            <Link to={`/substates?address=${encodeURIComponent(event.substate_id)}`}>
                              {truncateText(event.substate_id, 20)}
                            </Link>
                            <CopyToClipboard copy={event.substate_id} />
                          </>
                        ) : (
                          truncateText(event.substate_id, 20)
                        )}
                      </TableCell>
                      <TableCell>
                        {event.template_address ? (
                          <>
                            <Link to={`/templates?address=${encodeURIComponent(event.template_address)}`}>
                              {truncateText(event.template_address, 20)}
                            </Link>
                            <CopyToClipboard copy={event.template_address} />
                          </>
                        ) : (
                          truncateText(event.template_address, 20)
                        )}
                      </TableCell>
                      <TableCell>
                        <Stack direction="row" spacing={2} alignItems="left">
                          <Button variant="outlined" onClick={() => handlePayloadView(event)}>
                            View
                          </Button>
                          <Button variant="outlined" onClick={() => handlePayloadDownload(txId, event)}>
                            Download
                          </Button>
                        </Stack>
                      </TableCell>
                    </TableRow>
                  ) : (
                    <TableRow key={i}>
                      <TableCell colSpan={5}>
                        {truncateText(txId, 20)}: CBOR, {encodedByteLength(encoded)} bytes
                      </TableCell>
                    </TableRow>
                  ),
                )
              )}
            </TableBody>
          </Table>
          <Stack direction="row" justifyContent="right" spacing={2} alignItems="center">
            <IconButton
              aria-label="previous page"
              disabled={loading || page === 0}
              onClick={() => handleChangePage(page - 1)}
            >
              <KeyboardArrowLeftIcon />
            </IconButton>
            <Typography sx={{}}>{page}</Typography>
            <IconButton
              aria-label="next page"
              disabled={loading || nextCursor === null}
              onClick={() => handleChangePage(page + 1)}
            >
              <KeyboardArrowRightIcon />
            </IconButton>
          </Stack>
        </StyledPaper>
      </Grid>
      <JsonDialog open={jsonDialogOpen} onClose={handleJsonDialogClose} data={selectedPayload} />
    </>
  );
}

export default EventsLayout;
