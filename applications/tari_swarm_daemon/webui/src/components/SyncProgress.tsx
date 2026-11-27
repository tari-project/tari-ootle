//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { CatchUp, Channel, SYNC_STATES } from "../consensus";
import { IndexerSyncStats } from "../types";
import { Tag } from "../ui";

/** A bar filled to `fraction` (0 to 1), or a moving stripe when `fraction` is null and progress cannot be measured. */
export function ProgressBar({ fraction, title }: { fraction: number | null; title?: string }) {
  const width = fraction === null ? undefined : `${Math.round(Math.min(1, Math.max(0, fraction)) * 100)}%`;
  return (
    <div className={fraction === null ? "progress indeterminate" : "progress"} title={title}>
      <div className="progress-fill" style={{ width }} />
    </div>
  );
}

/** How far a validator has synced: by blocks back to the tip while it catches up, or a stripe while it state-syncs. */
export function ValidatorSync({
  channel,
  catchUp,
  compact,
}: {
  channel: Channel;
  catchUp: CatchUp | undefined;
  compact?: boolean;
}) {
  if (SYNC_STATES.has(channel.state)) {
    return (
      <div className="sync">
        <ProgressBar fraction={null} title={channel.state} />
        {!compact && <span className="faint">{channel.state}</span>}
      </div>
    );
  }
  if (!catchUp) {
    return null;
  }
  const done = catchUp.from - catchUp.behind;
  const label = `caught up ${done} of ${catchUp.from} blocks · ${catchUp.behind} behind`;
  return (
    <div className="sync">
      <ProgressBar fraction={done / catchUp.from} title={label} />
      {!compact && <span className="faint">{label}</span>}
    </div>
  );
}

/**
 * How far one shard has synced, from 0 to 1. A shard taking a snapshot reports how much of its key space it has
 * paged; a shard streaming versions compares its verified version with the latest leaf it has seen.
 */
function shardCompletion(shard: IndexerSyncStats["shards"][number]): number {
  if (shard.state === "Following") {
    return 1;
  }
  if (shard.snapshot) {
    return shard.snapshot.fraction;
  }
  const tip = shard.tip_leaf?.state_version ?? 0;
  if (tip === 0) {
    return 0;
  }
  return Math.min(1, (shard.verified?.state_version ?? 0) / tip);
}

/** An indexer's sync progress across every shard it follows, with the phase of any shard group still syncing. */
export function IndexerSync({ stats }: { stats: IndexerSyncStats | null | undefined }) {
  if (!stats) {
    return <span className="faint">Sync status unavailable</span>;
  }
  const total = stats.shards.length;
  const following = stats.shards.filter((s) => s.state === "Following").length;
  const halted = stats.shards.filter((s) => s.state === "Halted").length;
  if (total > 0 && following === total) {
    return (
      <Tag tone="up" dot>
        synced · {total} shards following
      </Tag>
    );
  }
  const fraction = total ? stats.shards.reduce((sum, s) => sum + shardCompletion(s), 0) / total : null;
  const phases = stats.shard_groups
    .filter((g) => g.phase.kind !== "Streaming")
    .map((g) => {
      const range = `${g.shard_group.start}–${g.shard_group.end_inclusive}`;
      const shard = g.phase.shard === undefined ? "" : ` shard ${g.phase.shard}`;
      return `${range}: ${g.phase.kind}${shard}`;
    });
  return (
    <div className="sync">
      <ProgressBar fraction={fraction} />
      <span className="faint">
        {following} of {total} shards following
        {halted > 0 && <span style={{ color: "var(--down)" }}>{` · ${halted} halted`}</span>}
        {phases.length > 0 && ` · ${phases.join(", ")}`}
      </span>
    </div>
  );
}
