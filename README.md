# chrome-agent-search

Local-first Rust CLI for web search and page reading, inspired by [`web-search-mcp`](https://github.com/mzxrai/mcp-webresearch) and powered by [`chrome-agent`](https://github.com/sderosiaux/chrome-agent).

It is a pure command-line tool. No MCP server is embedded.

## Features

- Read pages through Chrome Readability.
- Extract visible text from a CSS selector.
- Search with Bing browser form flow, Brave, and DuckDuckGo Lite.
- Merge and rerank results from all engines in auto mode.
- Fetch full content for search results with bounded parallel reads.
- Run local benchmark cases and emit JSONL metrics.

## Install

```sh
cargo install --git https://github.com/ominiverdi/chrome-agent-search
```

Requirements:

- `chrome-agent` in `PATH`
- Chromium/Chrome usable by `chrome-agent`

## Usage

```sh
chrome-agent-search read 'https://example.com/article'
chrome-agent-search text 'https://example.com/page' --selector article
chrome-agent-search search 'query' --limit 5
chrome-agent-search full 'query' --limit 3 --max-chars 8000 --concurrency 3
chrome-agent-search bench --cases benchmarks/cases.toml --concurrency 3
```

Use JSON output:

```sh
chrome-agent-search --json search 'query' --limit 5
```

Choose an engine:

```sh
chrome-agent-search search 'query' --engine auto
chrome-agent-search search 'query' --engine all
chrome-agent-search search 'query' --engine bing
chrome-agent-search search 'query' --engine brave
chrome-agent-search search 'query' --engine duckduckgo
```

Browser and timeout options pass through to `chrome-agent`:

```sh
chrome-agent-search --stealth search 'query'
chrome-agent-search --copy-cookies read 'https://example.com/account'
chrome-agent-search --headed --connect http://127.0.0.1:9222 read 'https://example.com'
chrome-agent-search --chrome-timeout 20 --command-timeout 30 --wait-timeout 5 search 'query'
chrome-agent-search full 'query' --read-timeout 20 --concurrency 3
```

## Benchmarks

```sh
cargo build --release
./target/release/chrome-agent-search bench \
  --cases benchmarks/cases.toml \
  --out benchmarks/results/local.jsonl
```

Benchmarks are for local regression testing and same-environment comparisons. Search results vary by time, region, IP reputation, browser state, and engine behavior.
