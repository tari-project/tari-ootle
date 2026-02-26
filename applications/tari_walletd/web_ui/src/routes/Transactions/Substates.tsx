//  Copyright 2022. The Tari Project
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

import CodeBlockExpand from "@components/CodeBlock";
import CopyAddress from "@components/CopyAddress";
import { AccordionIconButton, DataTableCell } from "@components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import { Box, Chip, Collapse, Table, TableBody, TableContainer, TableRow, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { Substate, SubstateId, substateIdToString, TransactionResult } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import { IoArrowDownCircle, IoArrowUpCircle } from "react-icons/io5";

function renderSubstateDetails(substate: any, id: SubstateId) {
  if (!substate || typeof substate === "number") {
    return null;
  }

  const substateObj = substate.substate || substate;

  if (substateObj?.NonFungible) {
    const nft = substateObj.NonFungible;
    return (
      <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Typography variant="subtitle2">NFT Details</Typography>

        {nft.data && (
          <Box>
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
              Immutable Data:
            </Typography>
            <Box sx={{ pl: 2 }}>
              {nft.data.Tag && nft.data.Tag[1]?.Map && (
                <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                  {nft.data.Tag[1].Map.map((item: any, index: number) => (
                    <Box key={index} sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <Chip
                        label={item[0]?.Text || JSON.stringify(item[0])}
                        size="small"
                        color="primary"
                        variant="outlined"
                      />
                      <Typography variant="body2">{item[1]?.Text || JSON.stringify(item[1])}</Typography>
                    </Box>
                  ))}
                </Box>
              )}
            </Box>
          </Box>
        )}

        {nft.mutable_data && (
          <Box>
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
              Mutable Data:
            </Typography>
            <Box sx={{ pl: 2 }}>
              {nft.mutable_data.Map && (
                <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                  {nft.mutable_data.Map.map((item: any, index: number) => (
                    <Box key={index} sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <Chip
                        label={item[0]?.Text || JSON.stringify(item[0])}
                        size="small"
                        color="secondary"
                        variant="outlined"
                      />
                      <Typography variant="body2">{item[1]?.Text || JSON.stringify(item[1])}</Typography>
                    </Box>
                  ))}
                </Box>
              )}
            </Box>
          </Box>
        )}
      </Box>
    );
  }

  if (substateObj?.Vault) {
    const vault = substateObj.Vault;
    return (
      <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Typography variant="subtitle2">Vault Details</Typography>

        {vault.resource_container && (
          <Box>
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
              Resource Container:
            </Typography>
            <Box sx={{ pl: 2 }}>
              {vault.resource_container.Confidential && (
                <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                  <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                    <Typography variant="body2" color="text.secondary">
                      Address:
                    </Typography>
                    <CopyAddress address={vault.resource_container.Confidential.address} />
                  </Box>
                  <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                    <Typography variant="body2" color="text.secondary">
                      Revealed Amount:
                    </Typography>
                    <Chip
                      label={vault.resource_container.Confidential.revealed_amount}
                      size="small"
                      color="success"
                      variant="outlined"
                    />
                  </Box>
                </Box>
              )}
              {vault.resource_container.NonFungible && (
                <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                  <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                    <Typography variant="body2" color="text.secondary">
                      Address:
                    </Typography>
                    <CopyAddress address={vault.resource_container.NonFungible.address} />
                  </Box>
                  {vault.resource_container.NonFungible.token_ids && (
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <Typography variant="body2" color="text.secondary">
                        Token IDs:
                      </Typography>
                      <Box sx={{ display: "flex", flexWrap: "wrap", gap: 0.5 }}>
                        {vault.resource_container.NonFungible.token_ids.map((token: any, index: number) => (
                          <Chip
                            key={index}
                            label={token.Uint64 || JSON.stringify(token)}
                            size="small"
                            color="info"
                            variant="outlined"
                          />
                        ))}
                      </Box>
                    </Box>
                  )}
                </Box>
              )}
            </Box>
          </Box>
        )}
      </Box>
    );
  }

  if (substateObj?.Resource) {
    const resource = substateObj.Resource;
    return (
      <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Typography variant="subtitle2">Resource Details</Typography>

        <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1 }}>
          <Chip label={`Type: ${resource.resource_type}`} size="small" color="primary" variant="outlined" />
          <Chip label={`Supply: ${resource.total_supply}`} size="small" color="secondary" variant="outlined" />
        </Box>

        {resource.metadata && (
          <Box>
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
              Metadata:
            </Typography>
            <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1 }}>
              {Object.entries(resource.metadata).map(([key, value]) => (
                <Chip key={key} label={`${key}: ${value}`} size="small" variant="outlined" />
              ))}
            </Box>
          </Box>
        )}
      </Box>
    );
  }

  return null;
}

function SubstateRowData(
  {
    id,
    substate,
    state,
  }: {
    id: SubstateId;
    substate?: Substate | number;
    state: string;
  },
  index: number,
) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();

  const substateId = substateIdToString(id);
  return (
    <>
      <TableRow key={`${index}-1`}>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "flex-start",
              gap: "0.5rem",
            }}
          >
            {state === "Up" ? (
              <IoArrowUpCircle style={{ width: 22, height: 22, color: "#5F9C91" }} />
            ) : (
              <IoArrowDownCircle style={{ width: 22, height: 22, color: "#ECA86A" }} />
            )}
            {state}
          </div>
        </DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          {substateId}{" "}
          {substate !== null && substate !== undefined
            ? "v" + (typeof substate === "number" ? substate : substate.version)
            : ""}
        </DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen(!open);
            }}
          >
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
      </TableRow>
      <TableRow key={`${index}-2`}>
        <DataTableCell
          style={{
            paddingBottom: theme.spacing(1),
            paddingTop: 0,
            borderBottom: "none",
          }}
          colSpan={3}
        >
          <Collapse in={open} timeout="auto" unmountOnExit>
            <Box sx={{ p: 2, backgroundColor: theme.palette.accent.background, borderRadius: 1 }}>
              {renderSubstateDetails(substate, id) && <Box sx={{ mb: 2 }}>{renderSubstateDetails(substate, id)}</Box>}
              <CodeBlockExpand title="Raw Substate Data" content={{ substate, id }} />
            </Box>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

export default function Substates({ data }: { data: TransactionResult }) {
  if ("Reject" in data) {
    return null;
  }
  let up, down;
  if ("AcceptFeeRejectRest" in data) {
    up = data.AcceptFeeRejectRest[0].up_substates;
    down = data.AcceptFeeRejectRest[0].down_substates;
  } else {
    up = data.Accept.up_substates;
    down = data.Accept.down_substates;
  }

  return (
    <TableContainer>
      <Table>
        <TableBody>
          {up.map(([id, substate]: [SubstateId, Substate | number], index: number) => {
            return <SubstateRowData id={id} substate={substate} state="Up" key={index} />;
          })}
          {down.map(([id, substate]: [SubstateId, Substate | number], index: number) => {
            return <SubstateRowData id={id} substate={substate} state="Down" key={index} />;
          })}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
