//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { swarmRpc } from "../api/rpc";
import ConsensusSpine from "../components/ConsensusSpine";
import PoolMatrix from "../components/PoolMatrix";
import { readChannels } from "../consensus";
import { useSwarm } from "../state/context";
import { ActionButton, Panel } from "../ui";

export default function Overview() {
  const swarm = useSwarm();
  const { channels, inStep } = readChannels(swarm.validators, swarm.details);
  const behind = channels.filter((c) => c.tone === "lag");
  const quiet = channels.filter((c) => c.tone === "off" || c.tone === "down");

  const headline = !channels.length
    ? "No validators are registered yet."
    : behind.length || quiet.length
      ? `${inStep} of ${channels.length} validators are in step` +
        (behind.length ? `, ${behind.map((c) => c.vn.name).join(", ")} behind` : "") +
        (quiet.length ? `, ${quiet.map((c) => c.vn.name).join(", ")} not reporting` : "")
      : "Every validator is on the same block.";

  return (
    <div className="stack">
      <div className="page-head">
        <div>
          <h1>Overview</h1>
          <p>{headline}</p>
        </div>
        <div className="row">
          <ActionButton
            busyTitle="Starting — each validator compiles first if its executable is out of date"
            onAct={() =>
              swarm.act("Start all validators", () => swarmRpc("start_all", { instance_type: "TariValidatorNode" }))
            }
          >
            Start all validators
          </ActionButton>
          <ActionButton
            onAct={() =>
              swarm.act("Stop all validators", () => swarmRpc("stop_all", { instance_type: "TariValidatorNode" }))
            }
          >
            Stop all validators
          </ActionButton>
          <ActionButton
            className="btn primary"
            onAct={() =>
              swarm.act("Add validator", () =>
                swarmRpc("add_validator_node", { name: null, register: true, mine: false }),
              )
            }
          >
            Add validator
          </ActionButton>
        </div>
      </div>

      <Panel title="Consensus" flush>
        <ConsensusSpine validators={swarm.validators} details={swarm.details} />
      </Panel>

      <Panel title="Transaction pools" note="rows are transactions, columns are validators" flush>
        <PoolMatrix validators={swarm.validators} />
      </Panel>
    </div>
  );
}
