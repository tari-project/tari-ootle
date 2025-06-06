# Tari implementation

This is where you can find the cutting edge development of the Tari smart contract layer.

You can read about the technical specifications of the Ootle in the [RFCs](https://rfc.tari.com).

If you're looking for the core Tari base layer code, it's an [this repository](https://github.com/tari-project/tari)

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

This will start a Minotari base node, a Minotari console wallet, an Ootle validator node, wallet and indexer.
Additionally, it will automatically submit the validator node registration and mine blocks until the validator node is
active.

Open `http://localhost:8080` where you can administer the running instances, get links to the various web UIs and
JSON-RPC endpoints, view logs and more.

NOTE: `tari_swarm_daemon` is specifically for development/debugging and runs a complete local test network. Instructions
for running a wallet, indexer, or validator node are still in the works.

#### Creating a smart contract template

See the [tari-cli](https://github.com/tari-project/tari-cli) tool for details.

### Get airdropped base layer (Mino)Tari tokens to pay for fees

This is built into the testnet wallet and faucet tokens can be obtained from the wallet web UI.

### Claiming L1 burn Tari on the Ootle

The easiest way to do this is to click a button in the `tari_swarm_daemon` web UI.

After creating an account in the Ootle wallet. Provide the account name and the amount of Tari to burn to the swarm
daemon.
This creates the burn transaction on the Minotari wallet and provides a "burn proof".
You can then copy and past that burn proof into the Ootle wallet web UI using the "Claim Burn" dialog.

L1 Minotari coins are able to be burnt and claimed, the user may convert (1:1) these to Tari coins on the layer-2
network. The first step is to burn Minotari base layer funds making sure to include a claim public key.
A private claim key must be known to claim the funds on the layer-2 Tari network. Burning
yields the following data:

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

The user must create an account on the Tari network in the web UI or by using the `accounts create` wallet CLI command
prior to claiming.
See the previous section for details on creating an account. The public key of this account should be used as the claim
public key when
burning funds.

The user can then claim burn Tari on the second layer, as follows: create a new `.json` file, with path
`<JSON_FILE_TO_RETRIEVE_BURN_TARI>`

```
{
    "claim_public_key": <CLAIM_PUBLIC_KEY>,
    "transaction_id": <TRANSACTION_ID>,
    "commitment": <COMMITMENT>,
    "ownership_proof": <OWNERSHIP_PROOF>,
    "rangeproof": <RANGEPROOF>
}
```

then run the command

```
cargo run --bin tari_ootle_wallet_cli -- accounts claim-burn --account <ACCOUNT_NAME> --json <JSON_FILE_TO_RETRIEVE_BURN_TARI> --fee <FEE>
```
