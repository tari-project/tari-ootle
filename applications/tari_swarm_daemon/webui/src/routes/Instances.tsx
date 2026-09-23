//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useMemo, useState } from "react";
import { InstanceControls, LogLinks, Running } from "../components/NodeCard";
import { useSwarm } from "../state/context";
import { Copyable, Empty, Panel, Tag } from "../ui";

export default function Instances() {
  const swarm = useSwarm();
  const [query, setQuery] = useState("");
  const [runningOnly, setRunningOnly] = useState(false);

  const rows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return swarm.instances
      .filter((i) => !runningOnly || i.is_running)
      .filter((i) => !needle || i.name.toLowerCase().includes(needle) || i.instance_type.toLowerCase().includes(needle))
      .sort((a, b) => a.id - b.id);
  }, [swarm.instances, query, runningOnly]);

  return (
    <div className="stack">
      <div className="page-head">
        <div>
          <h1>Instances</h1>
          <p>Every process the swarm manages, running or not.</p>
        </div>
      </div>

      <Panel
        note={`${rows.length} of ${swarm.instances.length}`}
        actions={
          <>
            <input
              type="search"
              placeholder="Filter by name or type"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ width: 220 }}
            />
            <button className={`btn sm${runningOnly ? " primary" : ""}`} onClick={() => setRunningOnly(!runningOnly)}>
              Running only
            </button>
          </>
        }
        flush
      >
        {rows.length ? (
          <div className="scroll-x">
            <table>
              <thead>
                <tr>
                  <th className="num">#</th>
                  <th>Name</th>
                  <th>Type</th>
                  <th>State</th>
                  <th>Ports</th>
                  <th>Data directory</th>
                  <th>Logs</th>
                  <th>Controls</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((instance) => (
                  <tr key={instance.id}>
                    <td className="num">{instance.id}</td>
                    <td className="nowrap">{instance.name}</td>
                    <td className="mono muted nowrap">{instance.instance_type}</td>
                    <td>
                      <div className="controls">
                        <Running isRunning={instance.is_running} />
                        {instance.is_config_dirty && <Tag tone="lag">config changed</Tag>}
                      </div>
                    </td>
                    <td>
                      <div className="ports">
                        {Object.entries(instance.ports).map(([name, port]) => (
                          <span className="port" key={name}>
                            {name} {port}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td>
                      <Copyable value={instance.base_path} chars={12} />
                    </td>
                    <td>
                      <LogLinks files={swarm.logs[instance.id]} />
                    </td>
                    <td>
                      <InstanceControls id={instance.id} isRunning={instance.is_running} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty>Nothing matches that filter.</Empty>
        )}
      </Panel>
    </div>
  );
}
