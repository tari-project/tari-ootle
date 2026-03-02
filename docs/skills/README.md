# AI Coding Agent Skills for Tari Ootle

This folder contains comprehensive development guides ("skills") that give AI coding agents the context they need to generate correct Tari Ootle code — including accurate APIs, boilerplate, working examples, and common pitfalls.

All files share the same body content and differ only in their agent-specific header. Pick the file for your agent and copy it to the location your tool expects.

## Available Skills

| File | Agent | Description |
|------|-------|-------------|
| [`CLAUDE.md`](CLAUDE.md) | [Claude Code](https://claude.ai) | Claude Code / Anthropic Claude |
| [`CURSOR_RULES.md`](CURSOR_RULES.md) | [Cursor](https://cursor.com) | Cursor AI editor |
| [`COPILOT_INSTRUCTIONS.md`](COPILOT_INSTRUCTIONS.md) | [GitHub Copilot](https://github.com/features/copilot) | GitHub Copilot coding agent |
| [`WINDSURF_RULES.md`](WINDSURF_RULES.md) | [Windsurf](https://windsurf.com) | Windsurf (Cognition) |
| [`AIDER_CONVENTIONS.md`](AIDER_CONVENTIONS.md) | [Aider](https://aider.chat) | Aider CLI assistant |
| [`CODEX_RULES.md`](CODEX_RULES.md) | [OpenAI Codex](https://openai.com/codex) | OpenAI Codex CLI |
| [`AMP_AGENTS.md`](AMP_AGENTS.md) | [Amp](https://ampcode.com) | Amp coding agent |
| [`GEMINI_RULES.md`](GEMINI_RULES.md) | [Gemini CLI](https://github.com/google-gemini/gemini-cli) | Google Gemini CLI |
| [`ANTIGRAVITY_RULES.md`](ANTIGRAVITY_RULES.md) | [Antigravity](https://antigravity.dev) | Antigravity agent |

## Installation

### Claude Code

Copy the file to your project root (or `.claude/` subdirectory):

```bash
cp docs/skills/CLAUDE.md ./CLAUDE.md
```

Claude Code reads `CLAUDE.md` automatically at the start of every session.

### Cursor

Copy into Cursor's rules directory:

```bash
mkdir -p .cursor/rules
cp docs/skills/CURSOR_RULES.md .cursor/rules/tari-ootle.md
```

Or place it as `AGENTS.md` in the project root — Cursor reads that too.

### GitHub Copilot

Copy into the `.github/` directory:

```bash
mkdir -p .github
cp docs/skills/COPILOT_INSTRUCTIONS.md .github/copilot-instructions.md
```

Copilot's coding agent reads `.github/copilot-instructions.md` automatically.

### Windsurf

Copy to the project root:

```bash
cp docs/skills/WINDSURF_RULES.md .windsurfrules
```

Or use `AGENTS.md` in the project root — Windsurf supports both.

### Aider

Load it as a read-only context file:

```bash
aider --read docs/skills/AIDER_CONVENTIONS.md
```

To load automatically on every run, add to `.aider.conf.yml`:

```yaml
read: docs/skills/AIDER_CONVENTIONS.md
```

### OpenAI Codex

Copy to the project root as `AGENTS.md`:

```bash
cp docs/skills/CODEX_RULES.md ./AGENTS.md
```

Codex reads `AGENTS.md` automatically before starting work. You can also place it at `~/.codex/AGENTS.md` for global defaults.

### Amp

Copy to the project root as `AGENTS.md`:

```bash
cp docs/skills/AMP_AGENTS.md ./AGENTS.md
```

Amp reads `AGENTS.md` automatically from the project root and subdirectories.

### Gemini CLI

Copy to the project root as `AGENTS.md`:

```bash
cp docs/skills/GEMINI_RULES.md ./AGENTS.md
```

Or configure in `.gemini/settings.json`:

```json
{ "contextFileName": "docs/skills/GEMINI_RULES.md" }
```

### Antigravity

Follow Antigravity's documentation for loading custom rules, using `ANTIGRAVITY_RULES.md` as the source.

## What's Covered

Each skill document includes:

- **Overview** — Key concepts (templates, components, resources, vaults, buckets, transactions)
- **Template authoring** — Project setup, `#[template]` macro, constructors, state rules, error handling
- **Resources** — All 4 types (fungible, non-fungible, confidential, stealth), `ResourceBuilder` full API
- **Vault & Bucket** — Complete method reference with examples
- **Access rules** — `rule!` macro syntax, `ComponentAccessRules`, `OwnerRule`, `CallerContext`
- **Cross-component calls** — `ComponentManager::get`, `.call()`, `.invoke()`
- **Events & randomness** — `emit_event`, `random_bytes`, `random_u32`
- **Publishing** — WASM compilation, wallet UI, and programmatic publishing via `ootle-rs`
- **Client-side Rust** — `ootle-rs` provider setup, `TransactionBuilder`, faucet, receipt parsing
- **Testing** — `tari_template_test_tooling` API (`TemplateTest`, `call_function`, `call_method`)
- **Wallet CLI** — `tari_ootle_wallet_cli` commands for accounts, transactions, keys
- **Complete examples** — Counter, token with admin badge, guessing game
- **Common mistakes** — Top 10 pitfalls and how to avoid them
