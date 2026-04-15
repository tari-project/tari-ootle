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

import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper } from "../../Components/StyledComponents";
import {
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
import { useEffect, useState } from "react";
import { truncateText } from "../../utils/helpers";
import KeyboardArrowLeftIcon from "@mui/icons-material/KeyboardArrowLeft";
import KeyboardArrowRightIcon from "@mui/icons-material/KeyboardArrowRight";
import { listWatchedTemplates, listWatchedSubstates } from "../../utils/api";
import CopyToClipboard from "../../Components/CopyToClipboard";
import { Link } from "react-router-dom";
import type { WatchedSubstateItem } from "@tari-project/ootle-ts-bindings";

const PAGE_SIZE = 20;

function WatchedSubstatesLayout() {
  const [templates, setTemplates] = useState<string[]>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(null);
  const [substates, setSubstates] = useState<WatchedSubstateItem[]>([]);
  const [page, setPage] = useState(0);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    listWatchedTemplates().then((resp) => {
      setTemplates(resp.templates);
      if (resp.templates.length > 0) {
        setSelectedTemplate(resp.templates[0]);
      }
      setLoading(false);
    });
  }, []);

  useEffect(() => {
    if (!selectedTemplate) return;
    fetchSubstates(selectedTemplate, 0);
  }, [selectedTemplate]);

  async function fetchSubstates(templateAddress: string, pageNum: number) {
    const resp = await listWatchedSubstates({
      template_address: templateAddress,
      limit: BigInt(PAGE_SIZE),
      offset: BigInt(pageNum * PAGE_SIZE),
    });
    setSubstates(resp.substates);
    setPage(pageNum);
  }

  async function handleChangePage(newPage: number) {
    if (!selectedTemplate || newPage < 0) return;
    await fetchSubstates(selectedTemplate, newPage);
  }

  function handleSelectTemplate(template: string) {
    setSelectedTemplate(template);
    setPage(0);
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
          <Typography variant="h6" sx={{ marginBottom: 2 }}>
            Watched Templates
          </Typography>
          {templates.length === 0 ? (
            <Typography color="textSecondary">No watched templates configured.</Typography>
          ) : (
            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
              {templates.map((t) => (
                <Chip
                  key={t}
                  label={truncateText(t, 24)}
                  color={selectedTemplate === t ? "primary" : "default"}
                  variant={selectedTemplate === t ? "filled" : "outlined"}
                  onClick={() => handleSelectTemplate(t)}
                  title={t}
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
            <Table sx={{ minWidth: 650 }} aria-label="watched substates table">
              <TableHead>
                <TableRow>
                  <TableCell>Component Address</TableCell>
                  <TableCell>Template Address</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {substates.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={2}>
                      <Typography color="textSecondary">No components found for this template.</Typography>
                    </TableCell>
                  </TableRow>
                ) : (
                  substates.map((s, i) => (
                    <TableRow key={i} sx={{ "&:last-child td, &:last-child th": { border: 0 } }}>
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
              <IconButton aria-label="previous page" onClick={() => handleChangePage(page - 1)} disabled={page === 0}>
                <KeyboardArrowLeftIcon />
              </IconButton>
              <Typography>{page}</Typography>
              <IconButton
                aria-label="next page"
                onClick={() => handleChangePage(page + 1)}
                disabled={substates.length < PAGE_SIZE}
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
