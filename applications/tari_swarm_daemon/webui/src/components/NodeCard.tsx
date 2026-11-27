//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { ReactNode } from "react";
import { Link } from "react-router-dom";
import { swarmRpc } from "../api/rpc";
import { InstanceLogs, useSwarm } from "../state/context";
import { ActionButton, Tag } from "../ui";

export function InstanceControls({ id, isRunning }: { id: number; isRunning: boolean }) {
  const swarm = useSwarm();
  return (
    <div className="controls">
      <ActionButton
        className="btn sm"
        disabled={isRunning}
        busyTitle="Starting — this compiles the executable first if it is out of date"
        onAct={() => swarm.act("Start", () => swarmRpc("start_instance", { by_id: id }))}
      >
        Start
      </ActionButton>
      <ActionButton
        className="btn sm"
        disabled={!isRunning}
        onAct={() => swarm.act("Stop", () => swarmRpc("stop_instance", { by_id: id }))}
      >
        Stop
      </ActionButton>
      <ActionButton
        className="btn sm danger"
        title="Stops the instance and deletes its data directory"
        onAct={async () => {
          if (window.confirm("Delete this instance's data? It will be stopped first.")) {
            await swarm.act("Delete data", () => swarmRpc("delete_data", { by_id: id }));
          }
        }}
      >
        Delete data
      </ActionButton>
    </div>
  );
}

function encodeLogPath(path: string): string | null {
  try {
    return encodeURIComponent(btoa(path));
  } catch {
    return null;
  }
}

function fileBasename(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

export function LogLinks({ files }: { files: InstanceLogs | undefined }) {
  const logs = files?.logs ?? [];
  const stdout = files?.stdout ?? [];

  if (!logs.length && !stdout.length) {
    return <span className="faint">No log files yet</span>;
  }
  return (
    <div className="links">
      {logs.map(([path]) => {
        const encoded = encodeLogPath(path);
        const label = fileBasename(path);
        return encoded ? (
          <Link key={`log:${path}`} to={`/log/${encoded}/normal`}>
            {label}
          </Link>
        ) : (
          <span key={`log:${path}`} className="faint" title={`Cannot open log path: ${path}`}>
            {label}
          </span>
        );
      })}
      {stdout.map(([path]) => {
        const encoded = encodeLogPath(path);
        const label = path.endsWith("stderr.log") ? "stderr" : "stdout";
        return encoded ? (
          <Link key={`stdout:${path}`} to={`/log/${encoded}/stdout`}>
            {label}
          </Link>
        ) : (
          <span key={`stdout:${path}`} className="faint" title={`Cannot open log path: ${path}`}>
            {label}
          </span>
        );
      })}
    </div>
  );
}

export function Running({ isRunning }: { isRunning: boolean }) {
  return (
    <Tag tone={isRunning ? "up" : "off"} dot={isRunning}>
      {isRunning ? "running" : "stopped"}
    </Tag>
  );
}

export function NodeCard({
  name,
  isRunning,
  id,
  links,
  children,
}: {
  name: string;
  isRunning: boolean;
  id: number;
  links?: ReactNode;
  children?: ReactNode;
}) {
  const swarm = useSwarm();
  return (
    <section className="panel">
      <header className="panel-head">
        <div className="row">
          <h2>{name}</h2>
          <Running isRunning={isRunning} />
        </div>
        <span className="faint mono">#{id}</span>
      </header>
      <div className="panel-body stack">
        {links && <div className="links">{links}</div>}
        {children}
        <div>
          <div className="eyebrow" style={{ marginBottom: 4 }}>
            Logs
          </div>
          <LogLinks files={swarm.logs[id]} />
        </div>
        <InstanceControls id={id} isRunning={isRunning} />
      </div>
    </section>
  );
}
