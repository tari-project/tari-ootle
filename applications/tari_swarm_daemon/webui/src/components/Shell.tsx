//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { ReactNode, useEffect, useId, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { swarmRpc } from "../api/rpc";
import { readChannels } from "../consensus";
import { useSwarm } from "../state/context";
import { ActionButton, Live, Segmented, Tag } from "../ui";

const RATES = [
  { label: "1s", value: 1000 },
  { label: "2s", value: 2000 },
  { label: "5s", value: 5000 },
  { label: "off", value: 0 },
];

type Theme = "dark" | "light";

function readInitialTheme(): Theme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

function useTheme() {
  const [theme, setTheme] = useState<Theme>(readInitialTheme);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem("swarm.theme", theme);
    } catch {
      // Ignore storage write errors (e.g. quota exceeded or read-only storage).
    }
  }, [theme]);
  return { theme, setTheme };
}

function Readout({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="readout">
      <span className="k">{label}</span>
      <span className="v">{children}</span>
    </div>
  );
}

export default function Shell() {
  const swarm = useSwarm();
  const { theme, setTheme } = useTheme();
  const refreshLabelId = useId();
  const themeLabelId = useId();
  const { channels, tipEpoch, tipHeight, inStep } = readChannels(swarm.validators, swarm.details);

  const running = swarm.validators.filter((v) => v.is_running).length;
  const allInStep = channels.length > 0 && inStep === channels.length;

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <b>Swarm</b>
          <span>Ootle</span>
        </div>

        <nav className="nav">
          <NavLink to="/" end className={({ isActive }) => (isActive ? "on" : "")}>
            Overview
          </NavLink>
          <NavLink to="/validators" className={({ isActive }) => (isActive ? "on" : "")}>
            Validators <span className="count">{swarm.validators.length}</span>
          </NavLink>
          <NavLink to="/wallets" className={({ isActive }) => (isActive ? "on" : "")}>
            Wallets <span className="count">{swarm.wallets.length}</span>
          </NavLink>
          <NavLink to="/indexers" className={({ isActive }) => (isActive ? "on" : "")}>
            Indexers <span className="count">{swarm.indexers.length}</span>
          </NavLink>
          <NavLink to="/base-layer" className={({ isActive }) => (isActive ? "on" : "")}>
            Base layer
          </NavLink>
          <NavLink to="/instances" className={({ isActive }) => (isActive ? "on" : "")}>
            Instances <span className="count">{swarm.instances.length}</span>
          </NavLink>
        </nav>

        <div className="sidebar-foot">
          <div className="sidebar-field" role="group" aria-labelledby={refreshLabelId}>
            <span id={refreshLabelId} className="eyebrow">
              Refresh every
            </span>
            <Segmented options={RATES} value={swarm.intervalMs} onChange={swarm.setIntervalMs} />
          </div>
          <div className="sidebar-field" role="group" aria-labelledby={themeLabelId}>
            <span id={themeLabelId} className="eyebrow">
              Theme
            </span>
            <Segmented
              options={[
                { label: "Dark", value: "dark" as Theme },
                { label: "Light", value: "light" as Theme },
              ]}
              value={theme}
              onChange={setTheme}
            />
          </div>
        </div>
      </aside>

      <div className="viewport">
        <header className="topbar">
          {allInStep ? (
            <Tag tone="up" dot>
              committee in step
            </Tag>
          ) : channels.length ? (
            <Tag tone="lag" dot>
              {inStep}/{channels.length} in step
            </Tag>
          ) : (
            <Tag tone="off">no committee</Tag>
          )}

          <span className="divider-v" />
          <Readout label="Epoch">
            <Live value={tipEpoch ?? "—"} />
          </Readout>
          <Readout label="Consensus">
            <Live value={tipHeight === null ? "—" : `h${tipHeight}`} />
          </Readout>
          <Readout label="Base layer">
            <Live value={swarm.baseNodeHeight === null ? "—" : `h${swarm.baseNodeHeight}`} />
          </Readout>
          <Readout label="Validators up">
            {running}/{swarm.validators.length}
          </Readout>

          <span className="divider-v" />
          <Tag tone={swarm.isMining ? "up" : "off"} dot={swarm.isMining}>
            {swarm.isMining ? "mining" : "not mining"}
          </Tag>

          <div className="grow" />
          <ActionButton
            className="btn sm"
            onAct={() => swarm.act("Mine a block", () => swarmRpc("mine", { num_blocks: 1 }))}
          >
            Mine 1 block
          </ActionButton>
          <button type="button" className="btn sm" onClick={swarm.refresh}>
            Refresh
          </button>
        </header>

        <main className="page">
          <Outlet />
        </main>
      </div>

      <div className="toasts">
        {swarm.toasts.map((toast) => (
          <div className="toast" key={toast.id} role="alert">
            <span className="msg">{toast.message}</span>
            <button type="button" className="btn ghost sm" onClick={() => swarm.dismiss(toast.id)}>
              Dismiss
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
