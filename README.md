# Tari Ootle 

This is where you can find the cutting-edge development of the Tari smart contract layer.

You can read about the technical specifications of the Ootle in the [RFCs](https://rfc.tari.com).

If you're looking for the core Tari base layer code, it's in [this repository](https://github.com/tari-project/tari)

## Tari Validator node

See the dedicated [README](./applications/tari_validator_node/README.md) for installation and running guides.

## Running and testing a validator

NOTE: This repo is heavily under development, so these instructions may change without notice.

The easiest way to run a test network is to use `tari_swarm_daemon`.

```shell
cargo run --bin tari_swarm_daemon --release -- -c data/swarm/config.toml init
# Edit your config. You may need to point it to the path for the tari L1 repo. By default it assumed it's checked out at `../tari`.
cargo run --bin tari_swarm_daemon --release -- -c data/swarm/config.toml start
```

This will start a Minotari base node, a Minotari console wallet, an Ootle validator node, a wallet and an indexer.
Additionally, it will automatically submit the validator node registration and mine blocks until the validator node is
active.

Open `http://localhost:8080` where you can administer the running instances, get links to the various web UIs and
JSON-RPC endpoints, view logs and more.

NOTE: `tari_swarm_daemon` is specifically for development/debugging and runs a complete local test network. Instructions
for running a wallet, indexer, or validator node, the feature is still in development.

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

> **Note:** These steps will likely be simplified in future releases.

1. **Run the Ootle wallet**  
   Start the Ootle wallet application in your environment.

2. **Generate a claim key**  
   - Use the Ootle wallet web UI **or** the `tari_ootle_wallet_cli` tool.
   - Store the claim private key securely and back it up. Do not share it.

3. **Open the L1 console wallet**  
   - Navigate to the **`burn`** tab.

4. **Burn the desired amount of Tari**  
   - Include the claim public key you generated in step 2.  
   - ⚠ **WARNING:** If you lose the claim private key (the secret), your funds will be permanently unclaimable.

5. **Copy the claim proof JSON** from the L1 console wallet:  

7. **Claim the burn**.  
   Use the Ootle wallet web UI or the `tari_ootle_wallet_cli` tool to claim the burn using the burn proof via the **"Claim Burn"** dialog.  



