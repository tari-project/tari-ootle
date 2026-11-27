//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { ValidatorNode, VnDetail } from "./types";
import { Tone } from "./ui/tone";

/** Height jitter of a block or two is normal propagation, not divergence. */
export const IN_STEP = 1;

/** How far behind the tip a validator is: by blocks, by whole epochs, or unknown. */
export type Deficit =
  | { kind: "none" }
  | { kind: "blocks"; n: number }
  | { kind: "epochs"; n: number }
  | { kind: "unknown" };

export interface Channel {
  vn: ValidatorNode;
  detail: VnDetail | undefined;
  epoch: number | null;
  height: number | null;
  state: string;
  deficit: Deficit;
  tone: Tone;
}

/**
 * Reads every validator against the committee tip. The tip is the highest block reached on
 * the highest epoch any validator reports, so agreement is a shared position and a lagging
 * validator has a measurable deficit from it.
 */
export function readChannels(validators: ValidatorNode[], details: Record<number, VnDetail>) {
  const rows = validators.map((vn) => {
    const detail = details[vn.instance_id];
    const consensus = vn.is_running ? (detail?.consensus ?? null) : null;
    return {
      vn,
      detail,
      epoch: consensus?.epoch ?? null,
      height: consensus?.height ?? null,
      state: !vn.is_running ? "Stopped" : (consensus?.state ?? "No data"),
    };
  });

  const live = rows.filter((r) => r.height !== null);
  const tipEpoch = live.length ? Math.max(...live.map((r) => r.epoch ?? 0)) : null;
  const atTip = live.filter((r) => r.epoch === tipEpoch);
  const tipHeight = atTip.length ? Math.max(...atTip.map((r) => r.height ?? 0)) : null;

  const channels: Channel[] = rows.map((r) => {
    if (r.height === null || tipEpoch === null || tipHeight === null) {
      return { ...r, deficit: { kind: "unknown" }, tone: r.state === "Stopped" ? "off" : "down" };
    }
    // Being on an older epoch is a different kind of behind: heights are not comparable.
    const epochsBehind = tipEpoch - (r.epoch ?? 0);
    if (epochsBehind > 0) {
      return { ...r, deficit: { kind: "epochs", n: epochsBehind }, tone: "lag" };
    }
    const blocks = Math.max(0, tipHeight - r.height);
    return {
      ...r,
      deficit: blocks > IN_STEP ? { kind: "blocks", n: blocks } : { kind: "none" },
      tone: blocks > IN_STEP ? "lag" : "up",
    };
  });

  const inStep = channels.filter((c) => c.tone === "up").length;
  return { channels, tipEpoch, tipHeight, inStep };
}

/** A lagging validator's progress back to the tip, measured from the largest deficit seen since it fell behind. */
export interface CatchUp {
  epoch: number;
  /** The most blocks the validator has been behind the tip since it fell behind in this epoch. */
  from: number;
  /** How many blocks it is behind now. */
  behind: number;
}

/**
 * Updates `previous` with this poll's deficits. A validator that is no longer behind by blocks drops out, and one that
 * fell further behind, or behind in a new epoch, restarts its measure from the new deficit.
 */
export function trackCatchUp(previous: Record<number, CatchUp>, channels: Channel[]): Record<number, CatchUp> {
  const next: Record<number, CatchUp> = {};
  for (const c of channels) {
    if (c.deficit.kind !== "blocks" || c.epoch === null) {
      continue;
    }
    const prior = previous[c.vn.instance_id];
    const from = prior && prior.epoch === c.epoch ? Math.max(prior.from, c.deficit.n) : c.deficit.n;
    next[c.vn.instance_id] = { epoch: c.epoch, from, behind: c.deficit.n };
  }
  return next;
}

/** Consensus states in which a validator syncs without a block height to measure progress by. */
export const SYNC_STATES = new Set(["CheckSync", "Syncing", "Presyncing"]);

export function deficitLabel(deficit: Deficit): string | null {
  switch (deficit.kind) {
    case "blocks":
      return `−${deficit.n}`;
    case "epochs":
      return `−${deficit.n} epoch${deficit.n > 1 ? "s" : ""}`;
    default:
      return null;
  }
}
