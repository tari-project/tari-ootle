//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import React, { useState } from "react";
import { jsonRpc } from "../utils/json_rpc.tsx";
import NodeControls from "./NodeControls.tsx";

interface Props {
  showLogs: boolean;
  autoRefresh: boolean;
}

export default function MinotariNodes(props: Props) {
  const [nodes, setNodes] = React.useState<null | [any]>(null);
  const [isLoading, setIsLoading] = React.useState(true);

  const reload = () =>
    jsonRpc("list_instances", { by_type: "MinoTariNode" }).then((nodes: any) => setNodes(nodes.instances));

  React.useEffect(() => {
    reload().then(() => setIsLoading(false));
  }, []);

  if (isLoading) {
    return <div>Loading...</div>;
  }

  return (
    <div>
      {nodes!.map((node: any, i: number) => (
        <Node key={i} {...node} autoRefresh={props.autoRefresh} onReload={reload} showLogs={props.showLogs} />
      ))}
    </div>
  );
}


function Node(props: any) {
  const [baseNode, setBaseNode] = useState<any>(null);
  const [isLoading, setIsLoading] = React.useState(false);
  const [error, setError] = useState<null | string>(null);

  const onStart = () => {
    jsonRpc("start_instance", { by_id: props.id })
      .then(props.onReload);
  };

  const onStop = () => {
    jsonRpc("stop_instance", { by_id: props.id })
      .then(props.onReload);
  };

  const onDeleteData = () => {
    jsonRpc("delete_instance_data", { instance_id: props.id })
      .then(props.onReload);
  };

  const load = () => {
    setError(null);
    return jsonRpc("get_minotari_node", { instance_id: props.id })
      .then((resp) => {
        setBaseNode(resp);
      }).catch((err) => {
        setError(err.message || err.toString());
      });
  };

  React.useEffect(() => {
    if (isLoading) {
      return;
    }
    setIsLoading(true);
    load().finally(() => setIsLoading(false));

    const timer = setInterval(() => {
      if (!isLoading && props.autoRefresh) {
        load();
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [props.id, props.autoRefresh]);

  return (
    <div className="info">
      <div>
        <b>Name</b>
        {props.name}
      </div>

      <div>
        <b>GRPC</b>
        {props.ports.grpc}
      </div>
      {error && <div className="error">{error}</div>}

      {isLoading ? <div>Loading...</div> :
        <div><b>Height:</b> {baseNode?.height}</div>}
      <NodeControls isRunning={props.is_running} onStart={onStart} onStop={onStop} onDeleteData={onDeleteData} />
      {props.showLogs && <div>TODO</div>}
    </div>
  );
}
