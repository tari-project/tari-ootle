//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { Theme } from "@mui/material/styles";
import type {
  ShardGroup,
  ShardGroupSyncPhase,
  ShardGroupSyncStatus,
  ShardSyncState,
  ShardSyncStatus,
  SyncActivityBucket,
} from "@tari-project/ootle-ts-bindings";

export type MapMode = "state" | "lag" | "activity";

/** A shard whose last mismatch is this recent is marked on the map. */
export const RECENT_MISMATCH_SECS = 10 * 60;

export const STATE_ORDER: ShardSyncState[] = ["Following", "CatchingUp", "Snapshotting", "Waiting", "Halted"];

export const STATE_LABEL: Record<ShardSyncState, string> = {
  Following: "Following",
  CatchingUp: "Catching up",
  Snapshotting: "Snapshotting",
  Waiting: "Waiting",
  Halted: "Halted",
};

export const STATE_DESCRIPTION: Record<ShardSyncState, string> = {
  Following: "Level with its committee, and the substate cache serves it",
  CatchingUp: "Streaming or verifying history its committee is ahead of",
  Snapshotting: "Indexing from a verified snapshot of the shard's tree",
  Waiting: "Nothing staged or verified yet",
  Halted: "Every committee member's history mismatched; stopped until the epoch moves",
};

export function stateColour(theme: Theme, state: ShardSyncState): string {
  switch (state) {
    case "Following":
      return theme.palette.success.main;
    case "CatchingUp":
      return theme.palette.info.main;
    case "Snapshotting":
      return theme.palette.primary.main;
    case "Halted":
      return theme.palette.error.main;
    case "Waiting":
      return theme.palette.text.disabled;
  }
}

/** Versions the shard's last verified version trails its latest probed tip leaf by. */
export function shardLag(shard: ShardSyncStatus): number | null {
  if (!shard.tip_leaf) return null;
  const verified = shard.verified?.state_version ?? -1;
  return Math.max(0, shard.tip_leaf.state_version - verified);
}

/** Versions staged above the shard's last verified version, awaiting a verified leaf. */
export function stagedBacklog(shard: ShardSyncStatus): number {
  if (!shard.staged) return 0;
  const verified = shard.verified?.state_version ?? -1;
  return Math.max(0, shard.staged.state_version - verified);
}

/** Maps `value` onto 0..1 on a log scale that saturates at `max`. */
export function logScale(value: number, max: number): number {
  if (value <= 0) return 0;
  return Math.min(1, Math.log10(1 + value) / Math.log10(1 + max));
}

export function hasRecentMismatch(shard: ShardSyncStatus, nowSec: number): boolean {
  return shard.last_mismatch !== null && nowSec - shard.last_mismatch.at_unix_s <= RECENT_MISMATCH_SECS;
}

export function formatShardGroup(sg: ShardGroup): string {
  return sg.start === sg.end_inclusive ? `Shard ${sg.start}` : `Shards ${sg.start}–${sg.end_inclusive}`;
}

export function groupContains(sg: ShardGroup, shard: number): boolean {
  return shard >= sg.start && shard <= sg.end_inclusive;
}

export function formatAge(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 2) return "just now";
  if (s < 60) return `${s}s ago`;
  const mins = Math.floor(s / 60);
  if (mins < 60) return `${mins}m ${s % 60}s ago`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ${mins % 60}m ago`;
}

export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s}s`;
  const mins = Math.floor(s / 60);
  if (mins < 60) return `${mins}m ${s % 60}s`;
  return `${Math.floor(mins / 60)}h ${mins % 60}m`;
}

export function formatRate(perSec: number): string {
  if (perSec === 0) return "0";
  if (perSec < 0.1) return perSec.toFixed(2);
  if (perSec < 10) return perSec.toFixed(1);
  return Math.round(perSec).toLocaleString();
}

export function phaseLabel(phase: ShardGroupSyncPhase): string {
  switch (phase.kind) {
    case "Starting":
      return "Starting";
    case "SyncingCheckpoints":
      return "Syncing checkpoints";
    case "Snapshotting":
      return `Snapshotting shard ${phase.shard}`;
    case "Streaming":
      return "Streaming";
    case "Paused":
      return "Paused";
    case "Halted":
      return `Halted on shard ${phase.shard}`;
  }
}

export function phaseColour(
  phase: ShardGroupSyncPhase,
): "success" | "info" | "primary" | "warning" | "error" | "default" {
  switch (phase.kind) {
    case "Streaming":
      return "success";
    case "SyncingCheckpoints":
      return "info";
    case "Snapshotting":
      return "primary";
    case "Paused":
      return "warning";
    case "Halted":
      return "error";
    case "Starting":
      return "default";
  }
}

/**
 * Per-second rates over the buckets of the last `windowSecs` that have closed, plus the bucket still filling
 * pro rata, so a rate does not dip every time a bucket opens.
 */
export function recentRate(
  activity: SyncActivityBucket[],
  bucketSecs: number,
  nowSec: number,
  windowSecs: number,
  pick: (b: SyncActivityBucket) => number,
): number {
  if (activity.length === 0) return 0;
  const from = nowSec - windowSecs;
  let total = 0;
  let span = 0;
  for (const bucket of activity) {
    const end = Math.min(bucket.start_unix_s + bucketSecs, nowSec);
    const start = Math.max(bucket.start_unix_s, from);
    if (end <= start) continue;
    total += pick(bucket) * ((end - start) / Math.max(1, end - bucket.start_unix_s));
    span += end - start;
  }
  return span > 0 ? total / span : 0;
}

/** The network's activity: each bucket summed across every shard group. */
export function networkActivity(groups: ShardGroupSyncStatus[]): SyncActivityBucket[] {
  const byStart = new Map<number, SyncActivityBucket>();
  for (const group of groups) {
    for (const bucket of group.activity) {
      const total = byStart.get(bucket.start_unix_s) ?? {
        start_unix_s: bucket.start_unix_s,
        versions: 0,
        transactions: 0,
      };
      total.versions += bucket.versions;
      total.transactions += bucket.transactions;
      byStart.set(bucket.start_unix_s, total);
    }
  }
  return [...byStart.values()].sort((a, b) => a.start_unix_s - b.start_unix_s);
}
