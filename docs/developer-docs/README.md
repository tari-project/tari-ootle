# Tari Dan Template Library Documentation Site

This repository contains the source code for the Tari Ootle Template Library documentation
site, built with Astro and Starlight.

## 🚀 Project Structure

The documentation content is located in `src/content/docs/`.
Images and other static assets are in `src/assets/` and `public/`.

## � AI Coding Agent Skills

If you are using **Claude Code** (or another AI coding agent) to build Ootle templates,
install the Ootle skill so the agent understands the SDK, CLI workflow, and common pitfalls:

```bash
# Install the Ootle skill for Claude Code
mkdir -p .claude/skills/ootle && curl -fsSo .claude/skills/ootle/SKILL.md \
  https://ootle.tari.com/.well-known/skills/claude-code/SKILL.md
```

Skills for other agents (Cursor, Windsurf, Aider) are listed at
[ootle.tari.com/llms.txt](https://ootle.tari.com/llms.txt).

## �🧞 Commands

All commands are run from the root of the project, from a terminal:

| Command        | Action                                                |
|:---------------|:------------------------------------------------------|
| `pnpm install` | Installs project dependencies                         |
| `pnpm dev`     | Starts a local development server at `localhost:4321` |
| `pnpm build`   | Builds the production documentation site to `./dist/` |
| `pnpm preview` | Previews the built site locally                       |
