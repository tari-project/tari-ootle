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

import type { GetEpochManagerStatsResponse, VNGetIdentityResponse } from "@tari-project/ootle-ts-bindings";
import { createContext, useEffect, useState } from "react";
import { Route, Routes } from "react-router-dom";
import BlockDetails from "./routes/Blocks/BlockDetails";
import Blocks from "./routes/Blocks/Blocks";
import CommitteeMembers from "./routes/Committees/CommitteeMembers";
import Connections from "./routes/Connections/Connections";
import Diagnostics from "./routes/Diagnostics/Diagnostics";
import ErrorPage from "./routes/ErrorPage";
import Fees from "./routes/Fees/Fees";
import Mempool from "./routes/Mempool/Mempool";
import TransactionDetails from "./routes/Transactions/TransactionDetails";
import ValidatorNodes from "./routes/ValidatorNodes/ValidatorNodes";
import TemplateFunctions from "./routes/VN/Components/TemplateFunctions";
import ValidatorNode from "./routes/VN/ValidatorNode";
import Layout from "./theme/LayoutMain";
import { getEpochManagerStats, getIdentity, getShardKey } from "./utils/json_rpc";

interface IContext {
  epoch?: GetEpochManagerStatsResponse;
  identity?: VNGetIdentityResponse;
  shardKey?: string | null;
  error?: string;
}

export const VNContext = createContext<IContext>({
  epoch: undefined,
  identity: undefined,
  shardKey: undefined,
  error: "",
});

export const breadcrumbRoutes = [
  {
    label: "Home",
    path: "/",
    dynamic: false,
  },
  {
    label: "Connections",
    path: "/connections",
    dynamic: false,
  },
  {
    label: "Fees",
    path: "/fees",
    dynamic: false,
  },
  {
    label: "Transactions",
    path: "/transactions",
    dynamic: false,
  },
  {
    label: "Blocks",
    path: "/blocks",
    dynamic: false,
  },
  {
    label: "Blocks",
    path: "/blocks/:blockId",
    dynamic: true,
  },
  {
    label: "Validator Nodes",
    path: "/vns",
    dynamic: false,
  },
  {
    label: "Mempool",
    path: "/mempool",
    dynamic: false,
  },
  {
    label: "Diagnostics",
    path: "/diagnostics",
    dynamic: false,
  },
  {
    label: "Transactions",
    path: "/transactions/:payloadId",
    dynamic: true,
  },
  {
    label: "Template",
    path: "/templates/:address",
    dynamic: true,
  },
  {
    label: "Committee",
    path: "/committees/:address",
    dynamic: true,
  },
];

export default function App() {
  const [epoch, setEpoch] = useState<GetEpochManagerStatsResponse | undefined>(undefined);
  const [identity, setIdentity] = useState<VNGetIdentityResponse | undefined>(undefined);
  const [shardKey, setShardKey] = useState<string | null>(null);
  const [epochError, setEpochError] = useState("");
  const [identityError, setIdentityError] = useState("");
  const [shardKeyError, setShardKeyError] = useState("");
  const error = epochError || identityError || shardKeyError;

  // Initial fetch and 2-minute epoch stats poll
  useEffect(() => {
    let cancelled = false;

    const refreshEpoch = () => {
      getEpochManagerStats()
        .then((response) => {
          if (cancelled) {
            return;
          }
          setEpochError("");
          setEpoch((prev) =>
            prev &&
            prev.current_epoch === response.current_epoch &&
            prev.current_block_height === response.current_block_height &&
            prev.current_block_hash === response.current_block_hash &&
            prev.is_valid === response.is_valid
              ? prev
              : response,
          );
        })
        .catch((reason) => {
          if (cancelled) {
            return;
          }
          console.error(reason);
          setEpochError("Json RPC error, please check console");
        });
    };

    refreshEpoch();
    getIdentity()
      .then((response) => {
        if (cancelled) {
          return;
        }
        setIdentityError("");
        setIdentity(response);
      })
      .catch((reason) => {
        if (cancelled) {
          return;
        }
        console.error(reason);
        setIdentityError("Json RPC error, please check console");
      });

    const id = window.setInterval(refreshEpoch, 2 * 60 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  // Get shard key.
  useEffect(() => {
    if (epoch === undefined || identity === undefined) {
      return;
    }
    let cancelled = false;
    getShardKey({
      epoch: epoch.current_epoch,
      public_key: identity.public_key,
    })
      .then((response) => {
        if (!cancelled) {
          setShardKeyError("");
          setShardKey(response.shard_key);
        }
      })
      .catch((reason) => {
        if (!cancelled) {
          console.error(reason);
          setShardKeyError("Json RPC error, please check console");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [epoch?.current_epoch, identity?.public_key]);

  return (
    <>
      <VNContext.Provider value={{ epoch, identity, shardKey, error }}>
        <Routes>
          <Route path="/" element={<Layout />} errorElement={<ErrorPage />}>
            <Route index element={<ValidatorNode />} />
            <Route path="connections" element={<Connections />} />
            <Route path="fees" element={<Fees />} />
            <Route path="blocks" element={<Blocks />} />
            <Route path="vns" element={<ValidatorNodes />} />
            <Route path="mempool" element={<Mempool />} />
            <Route path="diagnostics" element={<Diagnostics />} />
            <Route path="transactions/:transactionHash" element={<TransactionDetails />} />
            <Route path="blocks/:blockId" element={<BlockDetails />} />
            <Route path="templates/:address" element={<TemplateFunctions />} />
            <Route path="committees/:address" element={<CommitteeMembers />} />
            <Route path="*" element={<ErrorPage />} />
          </Route>
        </Routes>
      </VNContext.Provider>
    </>
  );
}
