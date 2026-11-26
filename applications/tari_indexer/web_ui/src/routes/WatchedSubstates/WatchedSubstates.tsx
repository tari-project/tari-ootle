//  Copyright 2026. The Tari Project
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
  Chip,
  IconButton,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import Grid from "@mui/material/Grid";
import type { WatchedSubstateItem, WatchedTemplateItem } from "@tari-project/ootle-ts-bindings";
import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import CopyToClipboard from "../../Components/CopyToClipboard";
import PageHeading from "../../Components/PageHeading";
import { StyledPaper } from "../../Components/StyledComponents";
import { listWatchedSubstates, listWatchedTemplates } from "../../utils/api";
import { truncateText } from "../../utils/helpers";

const PAGE_SIZE = 20;

function getErrorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error && err.message) {
    return err.message;
  }
  if (typeof err === "object" && err !== null && "message" in err && typeof err.message === "string" && err.message) {
    return err.message;
  }
  return fallback;
}

function WatchedSubstatesLayout() {
  const [templates, setTemplates] = useState<WatchedTemplateItem[]>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(null);
  const [substates, setSubstates] = useState<WatchedSubstateItem[]>([]);
  const [page, setPage] = useState(0);
  const [loading, setLoading] = useState(true);
  const [templatesError, setTemplatesError] = useState<string | null>(null);
  const [substatesError, setSubstatesError] = useState<string | null>(null);
  const [loadingSubstates, setLoadingSubstates] = useState(false);
  const requestIdRef = useRef(0);

  useEffect(() => {
    let cancelled = false;
    listWatchedTemplates()
      .then((resp) => {
        if (cancelled) return;
        setTemplates(resp.templates);
        setTemplatesError(null);
        if (resp.templates.length > 0) {
          setSelectedTemplate(resp.templates[0].template_address);
        }
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setTemplatesError(getErrorMessage(err, "Failed to load watched templates."));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const fetchSubstates = useCallback(async (templateAddress: string, pageNum: number) => {
    const reqId = ++requestIdRef.current;
    setLoadingSubstates(true);
    setSubstatesError(null);
    try {
      const resp = await listWatchedSubstates({
        template_address: templateAddress,
        limit: BigInt(PAGE_SIZE),
        offset: BigInt(pageNum * PAGE_SIZE),
      });
      if (reqId !== requestIdRef.current) return;
      setSubstates(resp.substates);
      setPage(pageNum);
    } catch (err: unknown) {
      if (reqId !== requestIdRef.current) return;
      setSubstatesError(getErrorMessage(err, "Failed to load watched substates."));
    } finally {
      if (reqId === requestIdRef.current) {
        setLoadingSubstates(false);
      }
    }
  }, []);

  useEffect(() => {
    if (!selectedTemplate) return;
    setSubstates([]);
    setPage(0);
    void fetchSubstates(selectedTemplate, 0);
    return () => {
      requestIdRef.current++;
    };
  }, [selectedTemplate, fetchSubstates]);

  async function handleChangePage(newPage: number) {
    if (!selectedTemplate || newPage < 0) return;
    await fetchSubstates(selectedTemplate, newPage);
  }

  function handleSelectTemplate(templateAddress: string) {
    if (templateAddress === selectedTemplate) {
      void fetchSubstates(templateAddress, 0);
      return;
    }
    setSelectedTemplate(templateAddress);
  }

  if (loading) {
    return (
      <Grid size={12}>
        <PageHeading>Watched Substates</PageHeading>
        <Typography>Loading...</Typography>
      </Grid>
    );
  }

  return (
    <>
      <Grid size={12}>
        <PageHeading>Watched Substates</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          <Typography variant="body2" color="textSecondary" sx={{ marginBottom: 3 }}>
            The indexer monitors configured template addresses for component creation events. When a transaction creates
            a component using a watched template, it is tracked here. This is useful for discovering components such as
            liquidity pools without scanning the entire substate space. Each indexer may configure different or no
            templates to watch.
          </Typography>
          <Typography variant="h6" sx={{ marginBottom: 2 }}>
            Watched Templates
          </Typography>
          {templatesError ? (
            <Alert severity="error">{templatesError}</Alert>
          ) : templates.length === 0 ? (
            <Typography color="textSecondary">No watched templates configured.</Typography>
          ) : (
            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
              {templates.map((t) => (
                <Chip
                  key={t.template_address}
                  label={t.template_name || truncateText(t.template_address, 24)}
                  color={selectedTemplate === t.template_address ? "primary" : "default"}
                  variant={selectedTemplate === t.template_address ? "filled" : "outlined"}
                  onClick={() => handleSelectTemplate(t.template_address)}
                  title={t.template_address}
                />
              ))}
            </Stack>
          )}
        </StyledPaper>
      </Grid>
      {selectedTemplate && (
        <Grid size={12}>
          <StyledPaper>
            <Typography variant="h6" sx={{ marginBottom: 2 }}>
              Components
            </Typography>
            {substatesError && (
              <Alert severity="error" sx={{ marginBottom: 2 }}>
                {substatesError}
              </Alert>
            )}
            <Table sx={{ minWidth: 650 }} aria-label="watched substates table">
              <TableHead>
                <TableRow>
                  <TableCell>Component Address</TableCell>
                  <TableCell>Template Address</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {substates.length === 0 ? (
                  loadingSubstates ? (
                    <TableRow>
                      <TableCell colSpan={2}>
                        <Typography color="textSecondary">Loading...</Typography>
                      </TableCell>
                    </TableRow>
                  ) : !substatesError ? (
                    <TableRow>
                      <TableCell colSpan={2}>
                        <Typography color="textSecondary">No components found for this template.</Typography>
                      </TableCell>
                    </TableRow>
                  ) : null
                ) : (
                  substates.map((s) => (
                    <TableRow key={s.component_address} sx={{ "&:last-child td, &:last-child th": { border: 0 } }}>
                      <TableCell>
                        <Link to={`/substates?address=${encodeURIComponent(s.component_address)}`}>
                          {truncateText(s.component_address, 40)}
                        </Link>
                        <CopyToClipboard copy={s.component_address} />
                      </TableCell>
                      <TableCell>
                        <Link to={`/templates?address=${encodeURIComponent(s.template_address)}`}>
                          {truncateText(s.template_address, 30)}
                        </Link>
                        <CopyToClipboard copy={s.template_address} />
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
            <Stack direction="row" justifyContent="right" spacing={2} alignItems="center">
              <IconButton
                aria-label="previous page"
                onClick={() => handleChangePage(page - 1)}
                disabled={loadingSubstates || page === 0}
              >
                <KeyboardArrowLeftIcon />
              </IconButton>
              <Typography>{page}</Typography>
              <IconButton
                aria-label="next page"
                onClick={() => handleChangePage(page + 1)}
                disabled={loadingSubstates || substates.length < PAGE_SIZE}
              >
                <KeyboardArrowRightIcon />
              </IconButton>
            </Stack>
          </StyledPaper>
        </Grid>
      )}
    </>
  );
}

export default WatchedSubstatesLayout;
