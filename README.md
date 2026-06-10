<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="100" alt="TokBar" />

# TokBar

**One dashboard for everything your AI coding agents spend.**

Tokens · Cost · Sessions · Models · Billing blocks, all parsed from local logs.
No account, no upload, fully offline.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey)
![Tauri](https://img.shields.io/badge/Tauri-v2-24C8DB?logo=tauri&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white)

**English** · [简体中文](README.zh-CN.md)

</div>

![Overview](docs/screenshots/overview.png)

| Trends | Models |
|:---:|:---:|
| ![Trends](docs/screenshots/trends.png) | ![Models](docs/screenshots/models.png) |

<details>
<summary><b>More screenshots: Billing Blocks</b></summary>

![Billing Blocks](docs/screenshots/blocks.png)

</details>

## What is TokBar?

TokBar is a cross-platform desktop app (macOS / Windows) that analyzes how you use AI coding agents on your machine: token usage, cost, requests, sessions, model distribution, agent distribution, and historical trends.

It is not a chat client. It is the dashboard for where your tokens and money go, and it lives in the menu bar so today's spend is always one glance away.

## Features

- **Multi-agent**: Claude Code, Codex CLI and Kimi CLI out of the box, with an adapter architecture ready for more
- **Accurate costs**: tiered pricing (>200k token brackets), 5m/1h cache-write pricing, cache-read discounts, fast/priority tier multipliers
- **Menu bar ticker**: today's cost or token count right next to the clock
- **5-hour billing blocks**: usage grouped into hour-aligned 5-hour windows matching Claude's session billing window, with live burn rate
- **Trends & breakdowns**: daily/weekly/monthly charts by agent, model, and token type
- **Local & private**: everything is parsed and stored locally in SQLite; nothing leaves your machine
- **Light & dark themes**, accent colors, and an English/中文 interface

## Supported data sources

| Agent | Location | Format |
|---|---|---|
| Claude Code | `$CLAUDE_CONFIG_DIR` / `~/.config/claude/projects` / `~/.claude/projects` | JSONL |
| Codex CLI (OpenAI) | `sessions/` and `archived_sessions/` under `$CODEX_HOME` | JSONL |
| Kimi CLI | `sessions/**/wire.jsonl` under `$KIMI_DATA_DIR` or `~/.kimi` | JSONL |

The adapter architecture (`src-tauri/src/adapters/`) makes it straightforward to add more agents (Gemini CLI, OpenCode, Copilot, …).

## Install

Download the latest installer from [**Releases**](https://github.com/peng2132/TokBar/releases):

- **macOS**: `*_aarch64.dmg` (Apple Silicon) or `*_x64.dmg` (Intel)
- **Windows**: `*_x64-setup.exe` or `*.msi`

> Builds are currently unsigned. On macOS, if you see "TokBar is damaged", run `xattr -cr /Applications/TokBar.app` once. On Windows, click "More info", then "Run anyway" on the SmartScreen prompt.

## Accuracy

Parsing and billing logic is ported from [ccusage](https://github.com/ryoppippi/ccusage), an implementation validated against large amounts of real-world data:

- Full `message.usage` token schema, including `cache_creation` `ephemeral_5m/1h` breakdowns
- Deduplication by `messageId + requestId`, keeping the record with more tokens on conflict
- LiteLLM pricing (embedded offline snapshot + online refresh) with three-level model-name matching
- Cost modes: `auto` (prefer logged costUSD) / `calculate` (always recompute) / `display` (logged costUSD only)
- Per-model token counts and costs reconcile to the cent with the official ccusage CLI

## Development

```bash
pnpm install
pnpm tauri dev      # development mode
pnpm tauri build    # package (macOS .app/.dmg, Windows .msi/.exe)
```

End-to-end backend test (scans real local data):

```bash
cd src-tauri && cargo test --test pipeline -- --nocapture
```

### Architecture

```
src-tauri/src/
├── adapters/        # one adapter per agent data source
│   ├── claude.rs    # Claude Code JSONL parsing
│   ├── codex.rs     # Codex CLI parsing
│   └── kimi.rs      # Kimi CLI parsing
├── pricing.rs       # LiteLLM pricing + model matching
├── cost.rs          # tiered cost calculation
├── db.rs            # SQLite incremental cache (skips unchanged files)
├── aggregate.rs     # daily/sessions/models/projects/blocks aggregation
├── types.rs         # normalized UsageRecord
└── lib.rs           # Tauri commands

src/
├── pages/           # Overview / Trends / Sessions / Models / Blocks / Settings
├── components/      # shadcn-style UI + Recharts charts
└── lib/             # api.ts (typed invoke) / format.ts / i18n.tsx
```

Data flow: on launch (and every 60s) incrementally scan log directories → parse & normalize → dedupe into SQLite (priced via LiteLLM on insert) → frontend queries aggregates by time range / cost mode.

## Credits

- [ccusage](https://github.com/ryoppippi/ccusage): TokBar's parsing and billing logic is ported from this project
- [Tauri](https://tauri.app) · [LiteLLM](https://github.com/BerriAI/litellm) (pricing data) · [Recharts](https://recharts.org)

## License

[MIT](LICENSE)
