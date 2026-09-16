//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useState } from "react";
import { swarmRpc } from "../api/rpc";
import { NodeCard } from "../components/NodeCard";
import { useSwarm } from "../state/context";
import { ActionButton, Empty, Field, Live, Panel, Tag } from "../ui";

const ONE_TARI = 1_000_000;

function Miner() {
  const swarm = useSwarm();
  const [blocks, setBlocks] = useState(1);
  const [seconds, setSeconds] = useState(120);

  return (
    <Panel
      title="Miner"
      actions={
        <Tag tone={swarm.isMining ? "up" : "off"} dot={swarm.isMining}>
          {swarm.isMining ? "mining on a timer" : "idle"}
        </Tag>
      }
    >
      <div className="stack">
        <p className="prose">
          Blocks confirm registrations and advance the base layer epoch. Mining past the epoch a
          validator activates in leaves the committee with no checkpoint to sync from, so mine in
          small steps.
        </p>

        <div className="row" style={{ alignItems: "flex-end" }}>
          <Field label="Blocks">
            <input
              type="number"
              min={1}
              value={blocks}
              onChange={(e) => setBlocks(Math.max(1, Number(e.target.value)))}
            />
          </Field>
          <ActionButton
            className="btn primary"
            busyTitle="Mining — the miner process runs until the blocks are found"
            onAct={() => swarm.act("Mine blocks", () => swarmRpc("mine", { num_blocks: blocks }))}
          >
            Mine {blocks} {blocks === 1 ? "block" : "blocks"}
          </ActionButton>
        </div>

        <div className="row" style={{ alignItems: "flex-end" }}>
          <Field label="Every (seconds)">
            <input
              type="number"
              min={1}
              value={seconds}
              disabled={swarm.isMining}
              onChange={(e) => setSeconds(Math.max(1, Number(e.target.value)))}
            />
          </Field>
          <ActionButton
            disabled={swarm.isMining}
            onAct={() =>
              swarm.act("Start mining", () =>
                swarmRpc("start_mining", { interval_seconds: seconds }),
              )
            }
          >
            Start timer
          </ActionButton>
          <ActionButton
            disabled={!swarm.isMining}
            onAct={() => swarm.act("Stop mining", () => swarmRpc("stop_mining", {}))}
          >
            Stop timer
          </ActionButton>
        </div>
      </div>
    </Panel>
  );
}

function BurnFunds() {
  const swarm = useSwarm();
  const [amount, setAmount] = useState(1000 * ONE_TARI);
  const [accountName, setAccountName] = useState("");
  const [claimUrl, setClaimUrl] = useState<string | null>(null);

  // burn_funds is addressed to the wallet daemon that holds the destination account — the daemon is queried
  // for the account's owner key. The swarm picks the console wallet the funds are spent from.
  const target = swarm.wallets.find((wallet) => wallet.is_running);
  if (!target) {
    return <span className="faint">Start a wallet daemon to burn funds into an account.</span>;
  }

  return (
    <div className="stack">
      <div className="eyebrow">Burn to {target.name}</div>
      <div className="row" style={{ alignItems: "flex-end" }}>
        <Field label="Amount in microTari">
          <input
            type="number"
            min={1}
            style={{ width: 140 }}
            value={amount}
            onChange={(e) => setAmount(Number(e.target.value))}
          />
        </Field>
        <Field label="Account name">
          <input
            type="text"
            placeholder="e.g. alice"
            value={accountName}
            onChange={(e) => setAccountName(e.target.value)}
          />
        </Field>
        <ActionButton
          disabled={!accountName}
          busyTitle="Burning — this mines 10 blocks"
          onAct={() =>
            swarm.act("Burn funds", async () => {
              const resp = await swarmRpc("burn_funds", {
                wallet_daemon_instance_id: target.instance_id,
                account_name: accountName,
                amount,
              });
              setClaimUrl(resp.url);
            })
          }
        >
          Burn funds
        </ActionButton>
      </div>
      <span className="faint">{(amount / ONE_TARI).toLocaleString()} tTARI. This mines 10 blocks.</span>
      {claimUrl && (
        <a href={claimUrl} target="_blank" rel="noreferrer">
          Claim proof
        </a>
      )}
    </div>
  );
}

export default function BaseLayer() {
  const swarm = useSwarm();
  const nodes = swarm.instances.filter((i) => i.instance_type === "MinoTariNode");
  const consoleWallets = swarm.instances.filter((i) => i.instance_type === "MinoTariConsoleWallet");

  return (
    <div className="stack">
      <div className="page-head">
        <div>
          <h1>Base layer</h1>
          <p>The Minotari chain the swarm registers validators on.</p>
        </div>
      </div>

      <div className="grid">
        {nodes.map((node) => (
          <NodeCard key={node.id} id={node.id} name={node.name} isRunning={node.is_running}>
            <dl className="kv">
              <dt>Height</dt>
              <dd className="num">
                <Live value={swarm.baseNodeHeight ?? "—"} />
              </dd>
              <dt>GRPC</dt>
              <dd className="num">{node.ports.grpc ?? "—"}</dd>
            </dl>
          </NodeCard>
        ))}

        {consoleWallets.map((wallet) => (
          <NodeCard key={wallet.id} id={wallet.id} name={wallet.name} isRunning={wallet.is_running}>
            <dl className="kv">
              <dt>GRPC</dt>
              <dd className="num">{wallet.ports.grpc ?? "—"}</dd>
            </dl>
            <BurnFunds />
          </NodeCard>
        ))}

        {!nodes.length && !consoleWallets.length && (
          <Empty>No base layer instances are configured.</Empty>
        )}
      </div>

      <Miner />
    </div>
  );
}
