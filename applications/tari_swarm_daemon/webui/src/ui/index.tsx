//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { ReactNode, useEffect, useRef, useState } from "react";

export type Tone = "up" | "lag" | "down" | "rail" | "off";

export function Tag({ tone = "off", dot, children }: { tone?: Tone; dot?: boolean; children: ReactNode }) {
  return (
    <span className={`tag ${tone}`}>
      {dot && <i className={`dot${tone === "up" ? " pulse" : ""}`} />}
      {children}
    </span>
  );
}

export function Panel({
  title,
  note,
  actions,
  flush,
  children,
}: {
  title?: ReactNode;
  note?: ReactNode;
  actions?: ReactNode;
  flush?: boolean;
  children: ReactNode;
}) {
  return (
    <section className="panel">
      {(title || actions) && (
        <header className="panel-head">
          <div className="row">
            {title && <h2>{title}</h2>}
            {note && <span className="faint mono">{note}</span>}
          </div>
          {actions && <div className="row">{actions}</div>}
        </header>
      )}
      <div className={`panel-body${flush ? " flush" : ""}`}>{children}</div>
    </section>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>;
}

/** Click-to-copy identifier. Shows the middle elided; copies the whole value. */
export function Copyable({ value, chars = 6 }: { value: string | null | undefined; chars?: number }) {
  const [said, setSaid] = useState(false);

  useEffect(() => {
    if (!said) {
      return;
    }
    const timer = window.setTimeout(() => setSaid(false), 1200);
    return () => window.clearTimeout(timer);
  }, [said]);

  if (!value) {
    return <span className="faint mono">—</span>;
  }
  const short = value.length > chars * 2 + 3 ? `${value.slice(0, chars)}…${value.slice(-chars)}` : value;

  return (
    <button
      type="button"
      className="copy"
      title={value}
      onClick={() => {
        void navigator.clipboard.writeText(value);
        setSaid(true);
      }}
    >
      <span className="truncate">{short}</span>
      <span className={`hint${said ? " said" : ""}`}>{said ? "copied" : "copy"}</span>
    </button>
  );
}

/** Flashes once whenever the value it shows changes, so a tick is visible at a glance. */
export function Live({ value }: { value: ReactNode }) {
  const previous = useRef(value);
  const [flash, setFlash] = useState(false);

  useEffect(() => {
    if (previous.current !== value) {
      previous.current = value;
      setFlash(true);
      const timer = window.setTimeout(() => setFlash(false), 600);
      return () => window.clearTimeout(timer);
    }
  }, [value]);

  return <span className={flash ? "tick" : undefined}>{value}</span>;
}

export function Segmented<T extends string | number>({
  options,
  value,
  onChange,
}: {
  options: { label: string; value: T }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="seg">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={option.value === value ? "on" : undefined}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

/**
 * A button for an action that blocks: starting an instance can take as long as a compile,
 * and mining blocks waits on a miner process. Shows progress for as long as the request is
 * in flight and refuses repeat clicks until it settles.
 */
export function ActionButton({
  className = "btn",
  disabled,
  title,
  busyTitle,
  onAct,
  children,
}: {
  className?: string;
  disabled?: boolean;
  title?: string;
  busyTitle?: string;
  onAct: () => Promise<unknown>;
  children: ReactNode;
}) {
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);

  useEffect(
    () => () => {
      mounted.current = false;
    },
    [],
  );

  const run = async () => {
    setBusy(true);
    try {
      await onAct();
    } finally {
      // The action may have removed this instance, taking the button with it.
      if (mounted.current) {
        setBusy(false);
      }
    }
  };

  return (
    <button
      type="button"
      className={className}
      disabled={disabled || busy}
      aria-busy={busy}
      title={busy ? (busyTitle ?? title) : title}
      onClick={() => void run()}
    >
      {busy && <i className="spinner" aria-hidden="true" />}
      {children}
    </button>
  );
}
