//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export interface Duration {
  secs: number;
  nanos: number;
}

export function displayDuration(duration: Duration): string {
  if (duration.secs === 0) {
    if (duration.nanos > 1000000) {
      return `${(duration.nanos / 1000000).toFixed(2)}ms`;
    }
    if (duration.nanos > 1000) {
      return `${(duration.nanos / 1000).toFixed(2)}µs`;
    }
    return `${duration.nanos}ns`;
  }
  if (duration.secs >= 60 * 60) {
    const hours = Math.floor(duration.secs / 3600);
    const minutes = Math.floor((duration.secs % 3600) / 60);
    return `${hours}h${minutes}m`;
  }
  if (duration.secs >= 60) {
    const minutes = Math.floor(duration.secs / 60);
    const secs = Math.floor(duration.secs % 60);
    return `${minutes}m${secs}s`;
  }
  return `${duration.secs}s`;
}
