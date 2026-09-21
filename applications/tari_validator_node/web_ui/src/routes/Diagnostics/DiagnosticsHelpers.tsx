//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { ReactNode } from "react";
import {
  IoAlertCircle,
  IoCube,
  IoFlash,
  IoGitNetwork,
  IoHardwareChip,
  IoInformationCircle,
  IoMedkit,
  IoSyncCircle,
  IoTime,
  IoWarning,
} from "react-icons/io5";
import type { DiagnosticLevel } from "@tari-project/ootle-ts-bindings";

export interface LevelStyle {
  label: string;
  color: string;
  background: string;
  icon: (size: number) => ReactNode;
}

export const LEVELS: Record<DiagnosticLevel, LevelStyle> = {
  error: {
    label: "Error",
    color: "#C0392B",
    background: "rgba(219, 126, 126, 0.16)",
    icon: (size) => <IoAlertCircle size={size} color="#C0392B" />,
  },
  warn: {
    label: "Warning",
    color: "#B26A00",
    background: "rgba(236, 168, 106, 0.2)",
    icon: (size) => <IoWarning size={size} color="#B26A00" />,
  },
  info: {
    label: "Info",
    color: "#40388A",
    background: "rgba(147, 48, 255, 0.1)",
    icon: (size) => <IoInformationCircle size={size} color="#40388A" />,
  },
};

export const LEVEL_ORDER: DiagnosticLevel[] = ["info", "warn", "error"];

/// The icon for a topic is chosen by its namespace, so a new event under a known namespace gets a
/// sensible icon without a code change here.
export function topicIcon(topic: string, size = 16): ReactNode {
  const namespace = topic.split(".")[0];
  switch (namespace) {
    case "consensus":
      return <IoGitNetwork size={size} />;
    case "sync":
      return <IoSyncCircle size={size} />;
    case "node":
      return <IoHardwareChip size={size} />;
    case "epoch":
      return <IoTime size={size} />;
    case "diagnostics":
      return <IoMedkit size={size} />;
    case "block":
      return <IoCube size={size} />;
    default:
      return <IoFlash size={size} />;
  }
}

export function formatTimestamp(millis: number): string {
  return new Date(millis).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function formatRelative(millis: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - millis) / 1000));
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  return `${Math.round(hours / 24)}d ago`;
}

/// Fields that name something the UI can link to.
export function fieldLink(key: string, value: string): string | null {
  switch (key) {
    case "block_id":
      return `/blocks/${value}`;
    case "transaction_id":
      return `/transactions/${value}`;
    default:
      return null;
  }
}
