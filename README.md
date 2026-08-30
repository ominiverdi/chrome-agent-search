# chrome-agent-search

Command-line web search and page reading built on top of [`chrome-agent`](https://github.com/sderosiaux/chrome-agent).

Scope:

- Pure CLI, no MCP server.
- Uses `chrome-agent` as the browser/page primitive.
- Retrieves evidence; it does not generate LLM summaries.

## Commands

```sh
chrome-agent-search read 'https://example.com/article'
chrome-agent-search text 'https://example.com/page' --selector article
chrome-agent-search search 'query' --limit 5
chrome-agent-search full 'query' --limit 3 --max-chars 8000
```

Use `--json` for machine-readable output:

```sh
chrome-agent-search search 'query' --limit 5 --json
```

## Status

Early prototype.

- `read` is useful now for direct URL reading via Chrome Readability.
- `text` is useful now for selector-scoped visible text.
- `search` uses auto engine selection: Bing first, Brave with stealth when Bing quality is low, DuckDuckGo as last fallback.
- `full` composes `search` + `read`.

## Requirements

- `chrome-agent` in `PATH`
- Chromium/Chrome usable by `chrome-agent`
