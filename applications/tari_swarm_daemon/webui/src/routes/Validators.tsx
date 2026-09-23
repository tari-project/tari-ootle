//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useEffect } from "react";
import { NavLink, useNavigate, useParams } from "react-router-dom";
import { swarmRpc } from "../api/rpc";
import { InstanceControls, LogLinks, Running } from "../components/NodeCard";
import { readChannels } from "../consensus";
import { distinguish } from "../names";
import { useSwarm } from "../state/context";
import { ActionButton, Copyable, Empty, Live, Panel, Tag } from "../ui";

function PoolTable({ validatorId }: { validatorId: number }) {
  const swarm = useSwarm();
  const pool = swarm.details[validatorId]?.pool ?? [];

  if (!pool.length) {
    return <Empty>Pool is empty.</Empty>;
  }
  return (
    <div className="scroll-x">
      <table>
        <thead>
          <tr>
            <th>Transaction</th>
            <th>Stage</th>
            <th>Ready</th>
          </tr>
        </thead>
        <tbody>
          {pool.map((record) => (
            <tr key={record.transaction_id}>
              <td>
                <Copyable value={record.transaction_id} />
              </td>
              <td className="mono">{record.stage}</td>
              <td>
                <Tag tone={record.is_ready ? "up" : "off"}>{record.is_ready ? "ready" : "waiting"}</Tag>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default function Validators() {
  const swarm = useSwarm();
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const { channels } = readChannels(swarm.validators, swarm.details);
  const label = distinguish(swarm.validators.map((v) => v.name));

  const selectedId = id ? Number(id) : swarm.validators[0]?.instance_id;
  const channel = channels.find((c) => c.vn.instance_id === selectedId);

  // Land on a real validator when the URL names one that has gone away.
  useEffect(() => {
    if (id && swarm.validators.length && !channel) {
      navigate("/validators", { replace: true });
    }
  }, [id, channel, swarm.validators.length, navigate]);

  if (!swarm.validators.length) {
    return (
      <div className="stack">
        <div className="page-head">
          <div>
            <h1>Validators</h1>
            <p>No validators yet. Add one to start a committee.</p>
          </div>
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
    );
  }

  const detail = channel ? swarm.details[channel.vn.instance_id] : undefined;
  const epoch = detail?.epoch;
  const committee = epoch?.committee_info;
  // The epoch manager's own answer to "am I in the active set this epoch", which is what
  // registration and exit gate on. Registration submitted but not yet activated reads as
  // false, so Register stays available until the validator is actually in.
  const isRegistered = epoch?.is_valid === true;

  return (
    <div className="stack">
      <div className="page-head">
        <div>
          <h1>Validators</h1>
          <p>Pick a validator to see its consensus state, identity and pool.</p>
        </div>
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

      <div style={{ display: "grid", gridTemplateColumns: "minmax(200px, 260px) 1fr", gap: 14, alignItems: "start" }}>
        <Panel flush>
          <nav className="nav" style={{ padding: 8 }}>
            {channels.map((c) => (
              <NavLink
                key={c.vn.instance_id}
                to={`/validators/${c.vn.instance_id}`}
                className={c.vn.instance_id === selectedId ? "on" : ""}
              >
                <span className="row" style={{ gap: 7, minWidth: 0 }}>
                  <i className="dot" style={{ color: `var(--${c.tone === "off" ? "off" : c.tone})` }} />
                  <span className="truncate" title={c.vn.name}>
                    {label(c.vn.name)}
                  </span>
                </span>
                <span className="count">{c.height === null ? "—" : `h${c.height}`}</span>
              </NavLink>
            ))}
          </nav>
        </Panel>

        {channel && (
          <div className="stack">
            <Panel
              title={channel.vn.name}
              actions={
                <>
                  <Running isRunning={channel.vn.is_running} />
                  <ActionButton
                    className="btn sm"
                    title={
                      !channel.vn.is_running
                        ? "Start the validator to submit a registration"
                        : isRegistered
                          ? "This validator is already in the active set"
                          : "Submit a registration for this validator. Mine a block to confirm it."
                    }
                    disabled={!channel.vn.is_running || isRegistered}
                    onAct={() =>
                      swarm.act("Register validator", () =>
                        swarmRpc("register_validator_node", {
                          instance_id: channel.vn.instance_id,
                          mine: false,
                        }),
                      )
                    }
                  >
                    Register
                  </ActionButton>
                  <ActionButton
                    className="btn sm"
                    title={
                      !channel.vn.is_running
                        ? "Start the validator to submit an exit"
                        : isRegistered
                          ? "Submit an exit for this validator. Mine a block to confirm it."
                          : "This validator is not in the active set"
                    }
                    disabled={!channel.vn.is_running || !isRegistered}
                    onAct={() =>
                      swarm.act("Submit exit", () =>
                        swarmRpc("exit_validator_node", {
                          instance_id: channel.vn.instance_id,
                          mine: false,
                        }),
                      )
                    }
                  >
                    Submit exit
                  </ActionButton>
                </>
              }
            >
              <div className="stack">
                {detail?.error && <Tag tone="down">{detail.error}</Tag>}
                <dl className="kv">
                  <dt>Consensus</dt>
                  <dd className="num">
                    <Live
                      value={
                        channel.height === null
                          ? "not reporting"
                          : `${channel.state} · epoch ${channel.epoch} · height ${channel.height}`
                      }
                    />
                    {channel.deficit.kind === "blocks" && (
                      <span style={{ color: "var(--lag)" }}>{`  ${channel.deficit.n} blocks behind the tip`}</span>
                    )}
                    {channel.deficit.kind === "epochs" && (
                      <span style={{ color: "var(--lag)" }}>
                        {`  ${channel.deficit.n} epoch${channel.deficit.n > 1 ? "s" : ""} behind the tip`}
                      </span>
                    )}
                  </dd>

                  <dt>Shard group</dt>
                  <dd className="num">
                    {committee
                      ? `${committee.shard_group.start}–${committee.shard_group.end_inclusive} · ${committee.num_shard_group_members} members`
                      : "not in a committee"}
                  </dd>

                  <dt>Base layer</dt>
                  <dd className="num">
                    {epoch ? (
                      <Live value={`epoch ${epoch.current_epoch} · scanned h${epoch.current_block_height}`} />
                    ) : (
                      "—"
                    )}
                    {epoch && (
                      <span className="faint">
                        {epoch.start_epoch === null ? "  inactive" : `  active since epoch ${epoch.start_epoch}`}
                      </span>
                    )}
                  </dd>

                  <dt>Public key</dt>
                  <dd>
                    <Copyable value={detail?.identity?.public_key} chars={10} />
                  </dd>

                  <dt>Peer id</dt>
                  <dd>
                    <Copyable value={detail?.identity?.peer_id} chars={10} />
                  </dd>

                  <dt>Endpoints</dt>
                  <dd className="links">
                    <a href={channel.vn.web} target="_blank" rel="noreferrer">
                      Web UI
                    </a>
                    <a href={channel.vn.jrpc} target="_blank" rel="noreferrer">
                      JSON-RPC
                    </a>
                  </dd>

                  <dt>Logs</dt>
                  <dd>
                    <LogLinks files={swarm.logs[channel.vn.instance_id]} />
                  </dd>
                </dl>

                <InstanceControls id={channel.vn.instance_id} isRunning={channel.vn.is_running} />
              </div>
            </Panel>

            <Panel title="Transaction pool" note={`${detail?.pool.length ?? 0} in pool`} flush>
              <PoolTable validatorId={channel.vn.instance_id} />
            </Panel>
          </div>
        )}
      </div>
    </div>
  );
}
