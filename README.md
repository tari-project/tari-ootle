# Tari Ootle 

This is where you can find the cutting-edge development of the Tari smart contract layer.

You can read about the technical specifications of the Ootle in the [RFCs](https://rfc.tari.com).

If you're looking for the core Tari base layer code, it's in [this repository](https://github.com/tari-project/tari)

## Accessing the Ootle Testnet

The Tari Ootle Wallet Daemon is available from the release page of the Ootle project: https://github.com/tari-project/tari-ootle/releases
Unzip the binaries, then run:

```shell
tari_ootle_walletd --network igor -b <yourdesiredconfigfolderpath>
```

This will start a wallet connected to the Igor Testnet. You can view the public Nodes here:

- Validator Node: http://18.217.22.26:12006
- Indexer: http://18.217.22.26:12502

Navigate to http://127.0.0.1:5100 to create an account, claim test tokens and start testing features.

## Running the Ootle Locally (Localnet Development Environment)

NOTE: This repo is heavily under development, so these instructions may change without notice.

The easiest way to test out the Ootle is to use the `tari_swarm_daemon`. This will spin up all necessary MinoTari and Ootle components for a localnet.

Clone both the tari and tari-ootle repositories in the same folder:
```shell
mkdir <containerfolder>
cd <containerfolder>
git clone https://github.com/tari-project/tari.git
git clone https://github.com/tari-project/tari-ootle.git ootle
```

So:
<Some container folder>
   | tari
   | ootle

`cd` into `tari` and change the branch `v4.9.0-pre.1`: 

```shell
cd tari
git fetch origin tag v4.9.0-pre.1
git checkout v4.9.0-pre.1
```

Once done, change directory to the `ootle` and run the following from the ootle folder:

```shell
rustup target add wasm32-unknown-unknown
cargo run --bin tari_swarm_daemon --release -- -c data/swarm/config.toml init
cargo run --bin tari_swarm_daemon --release -- -c data/swarm/config.toml start
```

This will get you an instance of the `tari_swarm_daemon`, starting a Minotari base node, a Minotari console wallet, an Ootle validator node, an Ootle wallet and an Indexer.
Additionally, it will automatically submit the validator node registration and mine blocks until the validator node is active.

Open `http://localhost:8080` where you can administer the running instances, get links to the various web UIs and JSON-RPC endpoints, view logs and more.

NOTE: `tari_swarm_daemon` is specifically for development/debugging and runs a complete local test network. Instructions for running a wallet, indexer, or validator node, the feature is still in development.

## Tari Validator node

See the dedicated [README](./applications/tari_validator_node/README.md) for installation and running guides.

#### Creating a smart contract template

See the [tari-cli](https://github.com/tari-project/tari-cli) tool for details.

### Get airdropped base layer (Mino)Tari tokens to pay for fees

This is built into the testnet wallet, and faucet tokens can be obtained from the wallet web UI.

### Claiming L1 burn Tari on the Ootle

L1 Minotari coins are able to be burnt and claimed, the user may convert (1:1) these to Tari coins on the layer-2
network.

The easiest way to do this is in a test environment to click a button in the `tari_swarm_daemon` web UI.

After creating an account in the Ootle wallet. Provide the account name and the amount of Tari to burn to the swarm
daemon. This creates the burn transaction on the Minotari wallet and provides a "burn proof".
You can then copy and paste that burn proof into the Ootle wallet web UI using the "Claim Burn" dialog.

For other environments, the "manual" process is as follows:

NOTE: These steps will likely be smoothed over in future apps.

1. Run the Ootle wallet,
2. Generate a key using the web ui or `tari_ootle_wallet_cli` tool.
3. Run a L1 console wallet and navigate to the `burn` tab.
4. Burn the desired amount of Tari, making sure to include the claim public key generated in step 2. WARNING: if you
   lose the claim public key, you will not be able to claim the funds on the Tari network.
5. Copy the claim proof JSON data from the L1 console wallet.

```
{
    "transaction_id": <transaction_id>,
    "is_success": <IS_SUCCESS>,
    "failure_message": <FAILURE_MESSAGE>,
    "commitment": <COMMITMENT>,
    "ownership_proof": <OWNERSHIP_PROOF>,
    "rangeproof": <RANGEPROOF>
}
```

6. Wait for the burn to be mined in. Validator nodes scan the L1 network for burnt UTXOs with special flags. Depending
   on the network configuration, this may require 10-100s of blocks before the burn is picked up.
7. Use the Ootle wallet web UI or the `tari_ootle_wallet_cli` tool to claim the burn using the burn proof using the "
   Claim burn" dialog.

