//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useEffect, useMemo, useRef, useState } from "react";
import { nodeRpc } from "../api/rpc";
import { distinguish } from "../names";
import { useSwarm } from "../state/context";
import { TxHolding, ValidatorNode } from "../types";
import { Copyable, Empty } from "../ui";

function decisionOf(record: { local_decision?: unknown; original_decision?: unknown }): string {
  const decision = record.local_decision ?? record.original_decision;
  if (typeof decision === "string") {
    return decision;
  }
  if (decision && typeof decision === "object" && "Abort" in decision) {
    return `Abort(${(decision as { Abort: unknown }).Abort})`;
  }
  if (decision && typeof decision === "object") {
    return "Commit";
  }
  return "—";
}

function Cell({ holding }: { holding: TxHolding }) {
  if (holding.kind === "pooled") {
    return (
      <span className="cell-in" title={`${holding.stage}${holding.ready ? " · ready" : ""}`}>
        {holding.stage.replace(/[a-z]/g, "").slice(0, 4) || holding.stage.slice(0, 4)}
      </span>
    );
  }
  if (holding.kind === "finalized") {
    return (
      <span className="cell-done" title={`Finalized: ${holding.decision}`}>
        {holding.decision.startsWith("Abort") ? "abrt" : "done"}
      </span>
    );
  }
  return (
    <span className="cell-gone" title="Not in this validator's pool and not known to it">
      —
    </span>
  );
}

/**
 * One row per transaction, one column per validator. A transaction that a validator is
 * missing shows as a break in its row, which is the cross-committee question this view
 * exists to answer.
 */
export default function PoolMatrix({ validators }: { validators: ValidatorNode[] }) {
  const swarm = useSwarm();
  const [resolved, setResolved] = useState<Record<string, TxHolding>>({});
  // Absences are stable, so a lookup is made once per validator/transaction pair.
  const asked = useRef(new Set<string>());

  const running = validators.filter((vn) => vn.is_running);
  const shortName = distinguish(validators.map((vn) => vn.name));

  const pools = useMemo(
    () =>
      running.map((vn) => ({
        vn,
        byId: new Map((swarm.details[vn.instance_id]?.pool ?? []).map((record) => [record.transaction_id, record])),
      })),
    [running, swarm.details],
  );

  const txIds = useMemo(() => {
    const ids = new Set<string>();
    for (const pool of pools) {
      for (const id of pool.byId.keys()) {
        ids.add(id);
      }
    }
    return [...ids].sort();
  }, [pools]);

  useEffect(() => {
    const lookups: Promise<void>[] = [];
    for (const { vn, byId } of pools) {
      for (const txId of txIds) {
        const key = `${vn.instance_id}:${txId}`;
        if (byId.has(txId) || asked.current.has(key)) {
          continue;
        }
        asked.current.add(key);
        lookups.push(
          nodeRpc(vn.jrpc, "get_transaction", [txId])
            .then((resp) => {
              const decision = resp?.transaction?.final_decision;
              setResolved((current) => ({
                ...current,
                [key]: decision
                  ? { kind: "finalized", decision: decisionOf({ local_decision: decision }) }
                  : { kind: "absent" },
              }));
            })
            .catch(() => {
              setResolved((current) => ({ ...current, [key]: { kind: "absent" } }));
            }),
        );
      }
    }
    void Promise.allSettled(lookups);
  }, [pools, txIds]);

  if (!txIds.length) {
    return <Empty>No transactions in any validator's pool.</Empty>;
  }

  return (
    <div className="scroll-x">
      <table className="matrix">
        <thead>
          <tr>
            <th>Transaction</th>
            {pools.map(({ vn }) => (
              <th className="cell" key={vn.instance_id} title={vn.name}>
                {shortName(vn.name)}
              </th>
            ))}
            <th>Decision</th>
          </tr>
        </thead>
        <tbody>
          {txIds.map((txId) => {
            const holder = pools.find((p) => p.byId.has(txId));
            const record = holder?.byId.get(txId);
            return (
              <tr key={txId}>
                <td>
                  <Copyable value={txId} />
                </td>
                {pools.map(({ vn, byId }) => {
                  const own = byId.get(txId);
                  const holding: TxHolding = own
                    ? { kind: "pooled", stage: own.stage, ready: own.is_ready }
                    : (resolved[`${vn.instance_id}:${txId}`] ?? { kind: "absent" });
                  return (
                    <td className="cell" key={vn.instance_id}>
                      <Cell holding={holding} />
                    </td>
                  );
                })}
                <td className="mono muted">{record ? decisionOf(record) : "—"}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
