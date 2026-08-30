# chrome-agent-search

Local-first Rust CLI for web search and page reading, inspired by [`web-search-mcp`](https://github.com/mrkrsl/web-search-mcp) and powered by [`chrome-agent`](https://github.com/sderosiaux/chrome-agent).

It is a pure command-line tool. No MCP server is embedded.

## Features

- Read pages through Chrome Readability.
- Extract visible text from a CSS selector.
- Search with Bing browser form flow, Brave, and DuckDuckGo Lite.
- Use fast adaptive auto search, with exhaustive merged search available as `--engine all`.
- Fetch full content for search results with bounded parallel reads.
- Run local benchmark cases and emit JSONL metrics.

## Install

Rust users:

```sh
cargo install chrome-agent --locked
cargo install --git https://github.com/ominiverdi/chrome-agent-search --locked
```

Prebuilt binaries are available on the GitHub Releases page.

Requirements:

- `chrome-agent` in `PATH`
- Chromium/Chrome usable by `chrome-agent`

See [`INSTALL.md`](INSTALL.md) for package/release install details.

## Usage

```sh
chrome-agent-search read 'https://example.com/article'
chrome-agent-search text 'https://example.com/page' --selector article
chrome-agent-search search 'query' --limit 5
chrome-agent-search full 'query' --limit 3 --max-chars 8000 --concurrency 3
chrome-agent-search bench --cases benchmarks/cases.toml --concurrency 3
chrome-agent-search cleanup --purge-orphans
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
chrome-agent-search --proxy-server socks5://127.0.0.1:9050 search 'query'
chrome-agent-search --headed --connect http://127.0.0.1:9222 read 'https://example.com'
chrome-agent-search --chrome-timeout 20 --command-timeout 30 --wait-timeout 5 search 'query'
chrome-agent-search full 'query' --read-timeout 20 --concurrency 3
```

`--proxy-server` only works for managed browsers started by `chrome-agent`; it cannot be combined with `--connect`.

Clean up managed browser sessions and profiles through `chrome-agent close`:

```sh
chrome-agent-search cleanup
chrome-agent-search cleanup --purge
chrome-agent-search cleanup --orphans
chrome-agent-search cleanup --purge-orphans
```

`--purge` deletes cookies/cache/profile data for the selected browser names.

## Benchmarks

```sh
cargo build --release
./target/release/chrome-agent-search bench \
  --cases benchmarks/cases.toml \
  --out benchmarks/results/local.jsonl
```

Benchmarks are for local regression testing and same-environment comparisons. Search results vary by time, region, IP reputation, browser state, and engine behavior.

Hard diagnostic cases live in `benchmarks/hard-cases.toml`. These are intentionally separate from the default smoke benchmark because sites such as Reddit may block automated access or return low-content pages.
