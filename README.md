# Tari Ootle

This is where you can find the cutting-edge development of the Tari smart contract layer.

You can read about the technical specifications of the Ootle in the [RFCs](https://rfc.tari.com).

If you're looking for the core Tari base layer code, it's in [this repository](https://github.com/tari-project/tari)

[Documentation](https://tari-project.github.io/tari-ootle/)

## Prerequisites

You will require the following tools and dependencies to successfully build the Ootle and/or run the Ootle locally via
the Localnet environment:

- **C/C++ compiler**
    - Linux: `gcc` or `clang`
    - macOS: `clang` (via Xcode CLI tools)
    - Windows: `MSVC` (via Visual Studio Build Tools)
- **Build tools**
    - `make`, `cmake`, or equivalent
    - `pkg-config` (Linux/macOS)
- **Libraries**
    - OpenSSL development libraries (`libssl-dev`)
    - SQLite development libraries (`libsqlite3-dev`)
    - Protobuf compiler (`protoc`) and headers (`libprotobuf-dev`)
- **Other**
    - `git`

- **Rust (=1.88)**: Install Rust using [rustup](https://rustup.rs), and add a WASM target:

```bash
# Install the rust version in rust-toolchain.toml
rustup install
# Add wasm target
rustup target add wasm32-unknown-unknown
```

### Web UIs

**Node.js (>=20.x) & npm**: Node.js is required for building the validator node, indexer and wallet web UIs.
NOTE: this is not required, and the binaries will still run and compile without building the web UIs.

Follow the instructions at [node.js](https://nodejs.org/en/download) for your desired operating system and
package/version managers.
We recommend installing Node.js via [`nvm`](https://github.com/nvm-sh/nvm) to easily manage versions across projects.

We use [pnpm](https://pnpm.io/installation) for package management.

## Accessing the Ootle Testnet

The Tari Ootle Wallet Daemon is available on the
project’s [releases page](https://github.com/tari-project/tari-ootle/releases).
Unzip the binaries, then run:

```shell
tari_ootle_walletd --network igor -b <yourdesiredconfigfolderpath>
```

This will start a wallet connected to the Igor Testnet. You can view the public Nodes here:

- Validator Node: [http://18.217.22.26:12006](http://18.217.22.26:12006)
- Indexer: [http://18.217.22.26:12502](http://18.217.22.26:12502)

Navigate to http://127.0.0.1:5100 to create an account, claim test tokens and start testing features.

## Running a Small Ootle Network Locally (Localnet)

NOTE: This repository is under heavy development. We'll try to keep instructions up to date, but they may become
outdated.

Confirm you have installed all the prerequisites listed in the **Prerequisites** section (Rust, Node.js, npm, linux
dependencies)

The easiest way to test out the Ootle is to use the `tari_swarm_daemon`. This will spin up all necessary MinoTari and
Ootle applications for a _localnet_ network.

Clone both the `tari` and `tari-ootle` repositories in the same folder:

```shell
mkdir <containerfolder>
cd <containerfolder>
git clone https://github.com/tari-project/tari.git
# Checkout the tag on the L1 repo that is recently tested. Check the workspace Cargo.toml in this repo for the correct tag if this one is outdated.
cd tari && git checkout v5.2.0-pre.5 && cd ..
git clone https://github.com/tari-project/tari-ootle.git ootle
```

```shell
cd tari
git checkout development
cd ../ootle
rustup target add wasm32-unknown-unknown
# Creates an initial "swarm" config in data/swarm/config.toml
cargo run --bin tari_swarm_daemon --release -- -c data/swarm/config.toml init
# Build all the necessary binaries (this may take a while) and starts the swarm
cargo run --bin tari_swarm_daemon --release -- -c data/swarm/config.toml start
```

> Note: For subsequent runs, you only need to run the third command with the `-k` argument to avoid trying to
> re-register the Validator Nodes: `cargo run --bin tari_swarm_daemon --release -- -c data/swarm/config.toml start -k`

This will get you an instance of the `tari_swarm_daemon`, starting a Minotari base node, a Minotari console wallet, an
Ootle validator node, an Ootle wallet and an Indexer.
Additionally, it will automatically submit the validator node registration and mine blocks until the validator node is
active.

Open `http://localhost:8080` where you can administer the running instances, get links to the various web UIs and
JSON-RPC endpoints, view logs and more.

NOTE: `tari_swarm_daemon` is specifically for development/debugging and runs a complete local test network. Instructions
for running a wallet, indexer, or validator node, the feature is still in development.

## Tari Validator node

See the dedicated [README](./applications/tari_validator_node/README.md) for installation and running guides.

#### Creating a smart contract template

See the [tari-cli](https://github.com/tari-project/tari-cli) tool for details.

## AI Coding Agent Skills

The [`docs/skills/`](docs/skills/) folder contains comprehensive development guides for building Tari Ootle templates and client applications. These skills are now published on the [documentation site](https://tari-project.github.io/tari-ootle/skills/) and discoverable by AI agents via the standard Agent Skills endpoint.

### Automatic Discovery

All skills are exposed via the standard Agent Skills Discovery endpoint:

```
https://tari-project.github.io/tari-ootle/.well-known/skills/
```

AI agents that support Agent Skills can automatically discover and load your skills using this URL. No manual setup required!

### Manual Installation (Optional)

If your tool doesn't support automatic discovery, copy the appropriate skill file to your agent's expected location:

| Agent | Source File | Copy To |
|-------|------------|---------|
| [Claude Code](https://claude.ai) | `docs/skills/claude-code/SKILL.md` | `CLAUDE.md` (project root or `.claude/`) |
| [Cursor](https://cursor.com) | `docs/skills/cursor/SKILL.md` | `.cursor/rules/tari-ootle.md` or `AGENTS.md` |
| [GitHub Copilot](https://github.com/features/copilot) | `docs/skills/github-copilot/SKILL.md` | `.github/copilot-instructions.md` |
| [Windsurf](https://windsurf.com) | `docs/skills/windsurf/SKILL.md` | `.windsurfrules` or `AGENTS.md` |
| [Aider](https://aider.chat) | `docs/skills/aider/SKILL.md` | Load via `aider --read docs/skills/aider/SKILL.md` |
| [OpenAI Codex](https://openai.com/codex) | `docs/skills/openai-codex/SKILL.md` | `AGENTS.md` (project root) |
| [Amp](https://ampcode.com) | `docs/skills/amp/SKILL.md` | `AGENTS.md` (project root) |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli) | `docs/skills/google-gemini/SKILL.md` | `AGENTS.md` (project root) |
| [Antigravity](https://antigravity.dev) | `docs/skills/antigravity/SKILL.md` | Per Antigravity docs |

These guides cover template authoring, resource management, access rules, client-side interaction with `ootle-rs`, testing with `tari_template_test_tooling`, the wallet CLI, and complete working examples.

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
    - If using the Ootle wallet, claim keys are derived from your wallet seed. Ensure you claim from the same
      seed/account later.
    - Record which claim public key you used for the burn. Keep your wallet seed/private key secure and never share it.

3. **Open the L1 console wallet**
    - Navigate to the **`burn`** tab.

4. **Burn the desired amount of Tari**
    - Include the claim public key you generated in step 2.
    - ⚠ **WARNING:** You must claim using the same claim public key that was included in the burn. If you don’t have
      access to the wallet/seed that can derive that key, you will not be able to claim the funds.

5. **Copy the claim proof JSON** from the L1 console wallet.

6. **Wait for the burn to be mined**.
    - Validator nodes scan the L1 network for burnt UTXOs with special flags.
    - Depending on the network configuration, this may take **tens to hundreds of blocks** before the burn is picked up.

7. **Claim the burn**.  
   Use the Ootle wallet web UI or the `tari_ootle_wallet_cli` tool to claim the burn using the burn proof via the **"
   Claim Burn"** dialog.
