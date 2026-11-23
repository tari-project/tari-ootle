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

import AddIcon from "@mui/icons-material/Add";
import { Alert, TextField } from "@mui/material";
import Button from "@mui/material/Button";
import Fade from "@mui/material/Fade";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import type { VNConnection } from "@tari-project/ootle-ts-bindings";
import React, { useCallback, useEffect, useRef, useState } from "react";
import { Form } from "react-router-dom";
import CopyToClipboard from "../../../Components/CopyToClipboard";
import { BoxHeading2, DataTableCell } from "../../../Components/StyledComponents";
import { displayDuration } from "../../../utils/helpers";
import { addPeer, getConnections } from "../../../utils/json_rpc";
import { shortenString } from "./helpers";

const useInterval = (fn: () => Promise<unknown>, ms: number) => {
  const timeout = useRef<number>(0);
  const mountedRef = useRef(false);
  const run = useCallback(async () => {
    try {
      await fn();
    } finally {
      if (mountedRef.current) {
        timeout.current = window.setTimeout(run, ms);
      }
    }
  }, [fn, ms]);
  useEffect(() => {
    mountedRef.current = true;
    run();
    return () => {
      mountedRef.current = false;
      window.clearTimeout(timeout.current);
    };
  }, [run]);
};

function Connections() {
  const [connections, setConnections] = useState<VNConnection[]>([]);
  const [showPeerDialog, setShowAddPeerDialog] = useState(false);
  const [formState, setFormState] = useState({ publicKey: "", address: "" });
  const [submitting, setSubmitting] = useState(false);
  const [fetchError, setFetchError] = useState<string>();
  const [addError, setAddError] = useState<string>();

  const showAddPeerDialog = (setElseToggle: boolean = !showPeerDialog) => {
    setAddError(undefined);
    setShowAddPeerDialog(setElseToggle);
  };

  const fetchConnections = useCallback(async () => {
    try {
      const resp = await getConnections();
      setConnections(resp.connections);
      setFetchError(undefined);
    } catch (err) {
      setFetchError(err instanceof Error ? err.message : String(err));
    }
  }, []);
  useInterval(fetchConnections, 5000);

  const onSubmitAddPeer = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const publicKey = formState.publicKey.trim();
    const address = formState.address.trim();
    if (submitting || !publicKey) {
      return;
    }
    setSubmitting(true);
    setAddError(undefined);
    try {
      await addPeer({
        public_key: publicKey,
        addresses: address ? [address] : [],
        wait_for_dial: false,
      });
      setFormState({ publicKey: "", address: "" });
      setShowAddPeerDialog(false);
      await fetchConnections();
    } catch (err) {
      setAddError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };
  const onChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setFormState({ ...formState, [e.target.name]: e.target.value });
    setAddError(undefined);
  };

  return (
    <>
      <BoxHeading2>
        {showPeerDialog && (
          <Fade in={showPeerDialog}>
            <Form onSubmit={onSubmitAddPeer} className="flex-container">
              <TextField
                name="publicKey"
                label="Public Key"
                value={formState.publicKey}
                onChange={onChange}
                style={{ flexGrow: 1 }}
                disabled={submitting}
              />
              <TextField
                name="address"
                label="Multiaddr"
                value={formState.address}
                onChange={onChange}
                style={{ flexGrow: 1 }}
                disabled={submitting}
              />
              <Button variant="contained" type="submit" disabled={submitting || !formState.publicKey.trim()}>
                Add Peer
              </Button>
              <Button variant="outlined" onClick={() => showAddPeerDialog(false)} disabled={submitting}>
                Cancel
              </Button>
            </Form>
          </Fade>
        )}
        {!showPeerDialog && (
          <Fade in={!showPeerDialog}>
            <div className="flex-container">
              <Button variant="outlined" startIcon={<AddIcon />} onClick={() => showAddPeerDialog()}>
                Add Peer
              </Button>
            </div>
          </Fade>
        )}
      </BoxHeading2>
      {addError && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {addError}
        </Alert>
      )}
      {fetchError && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {fetchError}
        </Alert>
      )}
      <TableContainer>
        <Table>
          <TableHead>
            <TableRow>
              <TableCell>Peer ID</TableCell>
              <TableCell>CopyAddress</TableCell>
              <TableCell>Age</TableCell>
              <TableCell>Direction</TableCell>
              <TableCell>Latency</TableCell>
              <TableCell>User Agent</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {connections &&
              connections.map(({ connection_id, address, age, direction, peer_id, ping_latency, user_agent }) => (
                <TableRow key={connection_id}>
                  <DataTableCell title={peer_id}>
                    {peer_id ? shortenString(peer_id) : "--"}
                    <CopyToClipboard copy={peer_id} />
                  </DataTableCell>
                  <DataTableCell>{address}</DataTableCell>
                  <DataTableCell>{displayDuration(age)}</DataTableCell>
                  <DataTableCell>{direction}</DataTableCell>
                  <DataTableCell>{ping_latency ? displayDuration(ping_latency) : "--"}</DataTableCell>
                  <DataTableCell>{user_agent ? user_agent.replace(/^\/tari\//, "") : "--"}</DataTableCell>
                </TableRow>
              ))}
          </TableBody>
        </Table>
      </TableContainer>
    </>
  );
}

export default Connections;
