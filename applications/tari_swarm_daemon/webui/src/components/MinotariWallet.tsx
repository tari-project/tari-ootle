//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import React from "react";
import { jsonRpc } from "../utils/json_rpc.tsx";
import NodeControls from "./NodeControls.tsx";

interface Props {
  showLogs: boolean;

}

export default function MinotariWallet(props: Props) {
  const [wallets, setWallets] = React.useState<null | [any]>(null);
  const [tariWallets, setTariWallets] = React.useState<null | [any]>(null);
  const [isLoading, setIsLoading] = React.useState(true);

  const reload = () =>
    jsonRpc("list_instances", { by_type: "MinoTariConsoleWallet" }).then((wallets: any) => setWallets(wallets.instances))
      .then(() => jsonRpc("list_instances", { by_type: "TariWalletDaemon" }).then((wallets: any) => setTariWallets(wallets.instances)))
      .then(() => setIsLoading(false));


  React.useEffect(() => {
    reload();
  }, []);

  if (isLoading) {
    return <div>Loading...</div>;
  }

  return (
    <div>
      {wallets!.map((wallet: any, i: number) => (
        <Wallet key={i} {...wallet} onReload={reload} showLogs={props.showLogs} tariWallets={tariWallets} />
      ))}
    </div>
  );
}

function Wallet(props: any) {
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

  const wallet = props.tariWallets[0];

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
      <NodeControls isRunning={props.is_running} onStart={onStart} onStop={onStop} onDeleteData={onDeleteData} />
      {(wallet) ?
        <BurnFunds instanceId={props.id} wallet={wallet} /> : <></>}
      {props.showLogs && <div>TODO</div>}
    </div>
  );
}

function BurnFunds(props: any) {
  const [amount, setAmount] = React.useState(1000 * 1000_000);
  const [accountName, setAccountName] = React.useState<null | string>(null);
  const [claimUrl, setClaimUrl] = React.useState<null | string>(null);

  const onBurnFunds = () => {
    jsonRpc("burn_funds", {
      wallet_instance_id: props.wallet.id,
      account_name: accountName,
      amount,
    }).then((res: any) => setClaimUrl(res.url));
  };

  return (
    <div>
      <pre>Burn to <b>{props.wallet.name}</b>. This will mine 10 blocks.</pre>
      <input type="number" value={amount} placeholder="amount"
             onChange={(e) => setAmount(parseInt(e.target.value, 10))} />
      <input type="text" value={accountName || ""} placeholder="account name"
             onChange={(e) => setAccountName(e.target.value)} />
      <button onClick={onBurnFunds}>Burn funds</button>
      {claimUrl && <div>Claim data: <a href={claimUrl} target="_blank">{claimUrl}</a></div>}
    </div>
  );
}