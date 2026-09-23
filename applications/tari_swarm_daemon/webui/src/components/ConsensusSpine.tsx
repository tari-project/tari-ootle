//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { Link } from "react-router-dom";
import { Deficit, IN_STEP, deficitLabel, readChannels } from "../consensus";
import { distinguish } from "../names";
import { ValidatorNode, VnDetail } from "../types";
import { Live, Tag } from "../ui";
import { toneColour } from "../ui/tone";

/** Headroom in the scale so the worst offender does not sit on the far edge. */
const MIN_SCALE = 8;
/** Where the rail sits across the track. */
const RAIL_PCT = 88;
/** Reserved for validators that are behind by a whole epoch, or reporting nothing. */
const OFF_SCALE_PCT = 3;
/** A validator behind by blocks never reaches the off-scale position. */
const FLOOR_PCT = 14;

/**
 * Validators as channels on a shared rail placed at the committee tip. Agreement reads as a
 * straight line into the rail; a lagging validator falls off it by the size of its deficit.
 */
export default function ConsensusSpine({
  validators,
  details,
}: {
  validators: ValidatorNode[];
  details: Record<number, VnDetail>;
}) {
  const { channels, tipEpoch, tipHeight } = readChannels(validators, details);

  if (!channels.length) {
    return <p className="empty">No validators yet. Add one to start a committee.</p>;
  }

  const label = distinguish(channels.map((c) => c.vn.name));
  const worst = Math.max(0, ...channels.map((c) => (c.deficit.kind === "blocks" ? c.deficit.n : 0)));
  const scale = Math.max(MIN_SCALE, worst * 1.5);

  // Distance left of the rail is the block deficit, drawn to scale.
  const positionOf = (deficit: Deficit) => {
    switch (deficit.kind) {
      case "none":
        return RAIL_PCT;
      case "blocks":
        return Math.max(FLOOR_PCT, RAIL_PCT * (1 - deficit.n / scale));
      default:
        return OFF_SCALE_PCT;
    }
  };

  return (
    <div className="spine">
      {channels.map((channel) => {
        const x = positionOf(channel.deficit);
        const unknown = channel.deficit.kind === "unknown";
        const colour = toneColour(channel.tone);
        return (
          <div className="spine-row" key={channel.vn.instance_id}>
            <div className="spine-name">
              <i className="dot" style={{ color: colour }} />
              <Link className="truncate" to={`/validators/${channel.vn.instance_id}`} title={channel.vn.name}>
                {label(channel.vn.name)}
              </Link>
            </div>

            <div>
              <Tag tone={channel.tone}>{channel.state}</Tag>
            </div>

            <div className="spine-track">
              <span className="spine-seg" style={{ left: `${RAIL_PCT}%` }} />
              {unknown ? (
                <span className="spine-line idle" style={{ left: 0, right: `${100 - RAIL_PCT}%` }} />
              ) : (
                <>
                  <span className={`spine-line ${channel.tone}`} style={{ left: 0, width: `${x}%` }} />
                  {x < RAIL_PCT && <span className="spine-gap" style={{ left: `${x}%`, width: `${RAIL_PCT - x}%` }} />}
                </>
              )}
              <span className="spine-head" style={{ left: `${x}%`, color: colour }} />
            </div>

            <div className="spine-tail num muted">
              {channel.height === null ? (
                "—"
              ) : (
                <>
                  <Live value={`e${channel.epoch} · h${channel.height}`} />
                  {deficitLabel(channel.deficit) && (
                    <span style={{ color: "var(--lag)" }}> {deficitLabel(channel.deficit)}</span>
                  )}
                </>
              )}
            </div>
          </div>
        );
      })}

      {/* Mirrors the row grid so the label lands under the rail at any width. */}
      <div className="spine-row spine-foot">
        <div />
        <div />
        <div className="spine-track">
          <span style={{ left: `${RAIL_PCT}%` }}>
            {tipHeight === null ? "no tip" : `tip e${tipEpoch} h${tipHeight}`}
          </span>
        </div>
        <div />
      </div>

      <div className="spine-legend">
        <span>
          <i className="dot" style={{ color: "var(--up)", display: "inline-block", marginRight: 5 }} />
          in step (within {IN_STEP} block)
        </span>
        <span>
          <i className="dot" style={{ color: "var(--lag)", display: "inline-block", marginRight: 5 }} />
          behind — the gap to the rail is the deficit, to scale
        </span>
        <span>
          <i className="dot" style={{ color: "var(--off)", display: "inline-block", marginRight: 5 }} />
          off scale: an epoch behind, or not reporting
        </span>
      </div>
    </div>
  );
}
