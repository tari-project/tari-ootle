//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useCallback, useEffect, useMemo, useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import CircularProgress from "@mui/material/CircularProgress";
import Collapse from "@mui/material/Collapse";
import Divider from "@mui/material/Divider";
import FormControlLabel from "@mui/material/FormControlLabel";
import Grid from "@mui/material/Grid";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import Link from "@mui/material/Link";
import Stack from "@mui/material/Stack";
import Switch from "@mui/material/Switch";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TextField from "@mui/material/TextField";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import {
  IoChevronDown,
  IoChevronForward,
  IoCheckmarkCircleOutline,
  IoRefresh,
  IoSearch,
  IoTrashOutline,
} from "react-icons/io5";
import type { DiagnosticEventRecord, DiagnosticLevel } from "@tari-project/ootle-ts-bindings";
import PageHeading from "../../Components/PageHeading";
import { StyledPaper } from "../../Components/StyledComponents";
import CopyToClipboard from "../../Components/CopyToClipboard";
import { clearDiagnosticEvents, getDiagnosticEvents } from "../../utils/json_rpc";
import theme from "../../theme/theme";
import { LEVELS, LEVEL_ORDER, fieldLink, formatRelative, formatTimestamp, topicIcon } from "./DiagnosticsHelpers";

const PAGE_SIZE = 50;
const AUTO_REFRESH_MS = 5000;

function LevelChip({ level }: { level: DiagnosticLevel }) {
  const style = LEVELS[level];
  return (
    <Chip
      size="small"
      icon={<Box sx={{ display: "flex", ml: "6px" }}>{style.icon(16)}</Box>}
      label={style.label}
      sx={{
        color: style.color,
        backgroundColor: style.background,
        fontWeight: 600,
        border: `1px solid ${style.color}22`,
      }}
    />
  );
}

function StatCard({ level, count }: { level: DiagnosticLevel; count: number }) {
  const style = LEVELS[level];
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
        gap: 1.5,
        px: 2.5,
        py: 1.5,
        borderRadius: `${theme.shape.borderRadius}px`,
        backgroundColor: style.background,
        border: `1px solid ${style.color}22`,
        minWidth: 150,
        flexGrow: 1,
      }}
    >
      {style.icon(26)}
      <Box>
        <Typography variant="h5" sx={{ color: style.color, lineHeight: 1.2, fontWeight: 700 }}>
          {count}
        </Typography>
        <Typography variant="caption" sx={{ color: style.color, opacity: 0.85 }}>
          {style.label}
          {count === 1 ? "" : "s"} in view
        </Typography>
      </Box>
    </Box>
  );
}

function FieldTable({ fields }: { fields: { [key in string]?: string } }) {
  const entries = Object.entries(fields).filter(([, value]) => value !== undefined) as [string, string][];
  if (entries.length === 0) {
    return (
      <Typography variant="body2" sx={{ fontStyle: "italic", opacity: 0.6 }}>
        No additional context was recorded for this event.
      </Typography>
    );
  }

  return (
    <Box sx={{ display: "grid", gridTemplateColumns: "max-content 1fr", columnGap: 3, rowGap: 1 }}>
      {entries.map(([key, value]) => {
        const link = fieldLink(key, value);
        return (
          <Box key={key} sx={{ display: "contents" }}>
            <Typography variant="body2" sx={{ fontWeight: 600, opacity: 0.7 }}>
              {key}
            </Typography>
            <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
              <Typography
                variant="body2"
                sx={{ fontFamily: "'Courier New', Courier, monospace", overflowWrap: "anywhere" }}
              >
                {link ? (
                  <Link component={RouterLink} to={link} underline="hover">
                    {value}
                  </Link>
                ) : (
                  value
                )}
              </Typography>
              <CopyToClipboard copy={value} />
            </Stack>
          </Box>
        );
      })}
    </Box>
  );
}

function EventRow({ event, now }: { event: DiagnosticEventRecord; now: number }) {
  const [open, setOpen] = useState(false);
  const style = LEVELS[event.level];

  return (
    <>
      <TableRow
        hover
        onClick={() => setOpen(!open)}
        sx={{
          "cursor": "pointer",
          "& > td": { borderBottom: open ? "none" : undefined },
          "borderLeft": `4px solid ${style.color}`,
        }}
      >
        <TableCell sx={{ width: 40, pr: 0 }}>
          <IconButton size="small" disableRipple aria-label={open ? "Collapse" : "Expand"}>
            {open ? <IoChevronDown size={16} /> : <IoChevronForward size={16} />}
          </IconButton>
        </TableCell>
        <TableCell sx={{ width: 130 }}>
          <LevelChip level={event.level} />
        </TableCell>
        <TableCell sx={{ width: 190, whiteSpace: "nowrap" }}>
          <Tooltip title={formatTimestamp(event.timestamp)} placement="top">
            <Typography variant="body2">{formatRelative(event.timestamp, now)}</Typography>
          </Tooltip>
        </TableCell>
        <TableCell sx={{ width: 260 }}>
          <Stack direction="row" spacing={1} alignItems="center">
            <Box sx={{ display: "flex", color: theme.palette.secondary.main }}>{topicIcon(event.topic)}</Box>
            <Typography variant="body2" sx={{ fontFamily: "'Courier New', Courier, monospace" }}>
              {event.topic}
            </Typography>
          </Stack>
        </TableCell>
        <TableCell>
          <Typography variant="body2">{event.message}</Typography>
        </TableCell>
      </TableRow>
      <TableRow>
        <TableCell colSpan={5} sx={{ py: 0, borderLeft: `4px solid ${style.color}` }}>
          <Collapse in={open} timeout="auto" unmountOnExit>
            <Box sx={{ px: 2, py: 2.5, backgroundColor: "#fafafa", borderRadius: 1, my: 1.5 }}>
              <FieldTable fields={event.fields} />
            </Box>
          </Collapse>
        </TableCell>
      </TableRow>
    </>
  );
}

export default function Diagnostics() {
  const [events, setEvents] = useState<DiagnosticEventRecord[]>([]);
  const [cursor, setCursor] = useState<number | null>(null);
  const [minLevel, setMinLevel] = useState<DiagnosticLevel | null>(null);
  const [topicPrefix, setTopicPrefix] = useState("");
  const [appliedTopicPrefix, setAppliedTopicPrefix] = useState("");
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());

  const filter = useMemo(
    () => ({
      min_level: minLevel,
      topic_prefix: appliedTopicPrefix.length > 0 ? appliedTopicPrefix : null,
      since: null,
      until: null,
    }),
    [minLevel, appliedTopicPrefix],
  );

  const load = useCallback(() => {
    return getDiagnosticEvents({ ...filter, before_id: null, limit: PAGE_SIZE })
      .then((response) => {
        setEvents(response.events);
        setCursor(response.next_cursor);
        setError(null);
        setNow(Date.now());
      })
      .catch((reason) => {
        console.error(reason);
        setError("Failed to load diagnostic events. Check the console for details.");
      })
      .finally(() => setIsLoading(false));
  }, [filter]);

  useEffect(() => {
    setIsLoading(true);
    load();
  }, [load]);

  // Refreshing re-fetches the first page, which would throw away everything "Load more" fetched, so
  // it pauses once the reader has paged back through history.
  const isPaged = events.length > PAGE_SIZE;

  useEffect(() => {
    if (!autoRefresh || isPaged) {
      return;
    }
    const id = window.setInterval(() => load(), AUTO_REFRESH_MS);
    return () => window.clearInterval(id);
  }, [autoRefresh, isPaged, load]);

  const loadMore = () => {
    if (cursor === null) {
      return;
    }
    setIsLoadingMore(true);
    getDiagnosticEvents({ ...filter, before_id: cursor, limit: PAGE_SIZE })
      .then((response) => {
        setEvents((current) => [...current, ...response.events]);
        setCursor(response.next_cursor);
      })
      .catch((reason) => {
        console.error(reason);
        setError("Failed to load more diagnostic events. Check the console for details.");
      })
      .finally(() => setIsLoadingMore(false));
  };

  const doClear = () => {
    setConfirmClear(false);
    clearDiagnosticEvents(filter)
      .then((response) => {
        setNotice(`Cleared ${response.deleted} event${response.deleted === 1 ? "" : "s"}.`);
        return load();
      })
      .catch((reason) => {
        console.error(reason);
        setError("Failed to clear diagnostic events. Check the console for details.");
      });
  };

  const counts = useMemo(() => {
    const initial: Record<DiagnosticLevel, number> = { info: 0, warn: 0, error: 0 };
    return events.reduce((acc, event) => {
      acc[event.level] += 1;
      return acc;
    }, initial);
  }, [events]);

  const isFiltered = minLevel !== null || appliedTopicPrefix.length > 0;

  return (
    <>
      <Grid size={12}>
        <PageHeading>Diagnostics</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <Stack spacing={3}>
            <Typography variant="body2" sx={{ opacity: 0.7 }}>
              Abnormal events this node has recorded — leader failures, no-votes, consensus errors, state sync and epoch
              changes. Kept locally and pruned automatically; nothing here is shared with the network.
            </Typography>

            <Stack direction="row" spacing={2} flexWrap="wrap" useFlexGap>
              {LEVEL_ORDER.slice()
                .reverse()
                .map((level) => (
                  <StatCard key={level} level={level} count={counts[level]} />
                ))}
            </Stack>

            <Divider />

            <Stack direction="row" spacing={2} alignItems="center" flexWrap="wrap" useFlexGap>
              <ToggleButtonGroup
                size="small"
                exclusive
                value={minLevel}
                onChange={(_event, value) => setMinLevel(value as DiagnosticLevel | null)}
                aria-label="Minimum level"
              >
                {LEVEL_ORDER.map((level) => (
                  <ToggleButton key={level} value={level} sx={{ gap: 0.75, px: 2 }}>
                    {LEVELS[level].icon(16)}
                    {LEVELS[level].label}
                    {level === "info" ? "" : "+"}
                  </ToggleButton>
                ))}
              </ToggleButtonGroup>

              <TextField
                size="small"
                placeholder="Filter by topic, e.g. consensus."
                value={topicPrefix}
                onChange={(event) => setTopicPrefix(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    setAppliedTopicPrefix(topicPrefix.trim());
                  }
                }}
                onBlur={() => setAppliedTopicPrefix(topicPrefix.trim())}
                slotProps={{
                  input: {
                    startAdornment: (
                      <InputAdornment position="start">
                        <IoSearch size={16} />
                      </InputAdornment>
                    ),
                  },
                }}
                sx={{ minWidth: 280 }}
              />

              <Box sx={{ flexGrow: 1 }} />

              <Tooltip title={isPaged ? "Paused while older pages are loaded" : ""}>
                <FormControlLabel
                  control={<Switch checked={autoRefresh} onChange={(event) => setAutoRefresh(event.target.checked)} />}
                  label={
                    <Typography variant="body2" sx={{ opacity: autoRefresh && isPaged ? 0.5 : 1 }}>
                      Auto-refresh
                      {autoRefresh && isPaged ? " (paused)" : ""}
                    </Typography>
                  }
                />
              </Tooltip>
              <Tooltip title="Refresh now">
                <IconButton onClick={() => load()} aria-label="Refresh">
                  <IoRefresh size={20} />
                </IconButton>
              </Tooltip>
              <Button
                variant="outlined"
                color="error"
                startIcon={<IoTrashOutline size={16} />}
                onClick={() => setConfirmClear(true)}
                disabled={events.length === 0}
              >
                Clear{isFiltered ? " filtered" : " all"}
              </Button>
            </Stack>

            {confirmClear && (
              <Alert
                severity="warning"
                action={
                  <Stack direction="row" spacing={1}>
                    <Button size="small" onClick={() => setConfirmClear(false)}>
                      Cancel
                    </Button>
                    <Button size="small" color="error" variant="contained" onClick={doClear}>
                      Delete
                    </Button>
                  </Stack>
                }
              >
                {isFiltered
                  ? "Permanently delete every event matching the current filters?"
                  : "Permanently delete every recorded event?"}
              </Alert>
            )}

            {notice && (
              <Alert severity="success" onClose={() => setNotice(null)}>
                {notice}
              </Alert>
            )}

            {error && (
              <Alert severity="error" onClose={() => setError(null)}>
                {error}
              </Alert>
            )}

            {isLoading ? (
              <Box sx={{ display: "flex", justifyContent: "center", py: 6 }}>
                <CircularProgress />
              </Box>
            ) : events.length === 0 ? (
              <Stack alignItems="center" spacing={1.5} sx={{ py: 6, opacity: 0.6 }}>
                <IoCheckmarkCircleOutline size={48} color={theme.palette.primary.main} />
                <Typography variant="h5">Nothing to report</Typography>
                <Typography variant="body2">
                  {isFiltered
                    ? "No events match the current filters."
                    : "This node has not recorded any diagnostic events."}
                </Typography>
              </Stack>
            ) : (
              <>
                <TableContainer>
                  <Table size="small">
                    <TableHead>
                      <TableRow>
                        <TableCell />
                        <TableCell>Level</TableCell>
                        <TableCell>When</TableCell>
                        <TableCell>Topic</TableCell>
                        <TableCell>Message</TableCell>
                      </TableRow>
                    </TableHead>
                    <TableBody>
                      {events.map((event) => (
                        <EventRow key={event.id} event={event} now={now} />
                      ))}
                    </TableBody>
                  </Table>
                </TableContainer>

                <Stack direction="row" spacing={2} alignItems="center" justifyContent="center">
                  {cursor !== null && (
                    <Button variant="outlined" onClick={loadMore} disabled={isLoadingMore}>
                      {isLoadingMore ? "Loading…" : "Load more"}
                    </Button>
                  )}
                  <Typography variant="caption" sx={{ opacity: 0.6 }}>
                    Showing {events.length} event{events.length === 1 ? "" : "s"}
                    {cursor === null ? " (all that match)" : ""}
                  </Typography>
                </Stack>
              </>
            )}
          </Stack>
        </StyledPaper>
      </Grid>
    </>
  );
}
