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

import { Alert, Grid } from "@mui/material";
import { CommitteeMember } from "@tari-project/ootle-ts-bindings";
import { useContext, useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { VNContext } from "../../App";
import PageHeading from "../../Components/PageHeading";
import { StyledPaper } from "../../Components/StyledComponents";
import { getCommittee } from "../../utils/json_rpc";
import Committee from "./CommitteeSingle";

async function getMembers(currentEpoch: number, shardKey: string) {
  const committee = await getCommittee({ epoch: currentEpoch, substate_address: shardKey });
  const committeeMembers = committee?.committee?.members;
  if (!committeeMembers || committeeMembers.length === 0) {
    throw new Error("Committee members not found");
  }
  return committeeMembers;
}

export default function CommitteeMembers() {
  const [members, setMembers] = useState<CommitteeMember<string>[]>([]);
  const [error, setError] = useState<string | null>(null);
  const { epoch, identity, shardKey } = useContext(VNContext);
  const { address } = useParams();
  const addresses = address ? address.split(",") : null;
  const hasValidRange = Boolean(addresses && addresses.length >= 2 && addresses[0] && addresses[1]);

  useEffect(() => {
    let cancelled = false;
    const fetchMembers = async () => {
      try {
        if (identity?.public_key && shardKey && epoch) {
          const committeeMembers = await getMembers(epoch.current_epoch, shardKey);
          if (!cancelled) {
            setMembers(committeeMembers);
            setError(null);
          }
        }
      } catch (err) {
        if (!cancelled) {
          setMembers([]);
          setError(err instanceof Error ? err.message : "Error fetching committee members");
        }
      }
    };
    fetchMembers();
    return () => {
      cancelled = true;
    };
  }, [epoch, identity?.public_key, shardKey]);

  if (epoch === undefined || identity === undefined) return <div>Loading</div>;

  return (
    <>
      <Grid size={12}>
        <PageHeading>Committee Members</PageHeading>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          {error && (
            <Alert severity="error" sx={{ mb: 2 }}>
              {error}
            </Alert>
          )}
          {hasValidRange && addresses ? (
            <Committee
              key={`${addresses[0]}-${addresses[1]}`}
              begin={addresses[0]}
              end={addresses[1]}
              members={members}
              publicKey={identity.public_key}
            />
          ) : (
            <Alert severity="warning">Invalid committee address range</Alert>
          )}
        </StyledPaper>
      </Grid>
    </>
  );
}
