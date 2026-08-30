# Benchmarks

This benchmark is a local regression suite for web search and page reading tools.
It checks whether a tool can find expected pages, extract useful snippets, read page content, and do it quickly.

It is not a universal search ranking benchmark. Results depend on location, IP reputation, browser state, time, network path, and search engine personalization. Use it to compare tools in the same environment and to catch regressions over time.

## Run

```sh
cargo build --release
./target/release/chrome-agent-search bench \
  --cases benchmarks/cases.toml \
  --out benchmarks/results/local.jsonl \
  --concurrency 3
```

Use a specific engine:

```sh
./target/release/chrome-agent-search bench --engine duckduckgo
./target/release/chrome-agent-search bench --engine brave
./target/release/chrome-agent-search bench --engine bing
```

Print machine-readable JSONL to stdout:

```sh
./target/release/chrome-agent-search --json bench --cases benchmarks/cases.toml --concurrency 3
```

## Case format

```toml
[[cases]]
id = "example-search"
kind = "search"
query = "site:example.com specific article title"
limit = 5
expected_urls = ["https://example.com/article"]
expected_domains = ["example.com"]
keywords = ["specific", "article"]

[[cases]]
id = "example-read"
kind = "read"
url = "https://example.com/article"
max_chars = 16000
min_chars = 1000
keywords = ["specific", "article"]
```

Supported kinds:

- `search`: run search and score result URLs/snippets.
- `read`: read one URL and score content length/keywords.
- `full`: search, read top results, and score combined content.

## Metrics

Each JSONL record includes:

- `ok`: case passed.
- `latency_seconds`: elapsed wall time.
- `result_count`: search result count, if applicable.
- `expected_url_rank`: 1-based rank of an expected URL, if found.
- `expected_domain_rank`: 1-based rank of an expected domain, if found.
- `keyword_coverage`: fraction of expected keywords found.
- `chars`: extracted content length, if applicable.
- `error`: failure reason, if any.

## Adding cases

Prefer public, stable pages. Do not add private URLs, secrets, paid full-text dumps, or local-only infrastructure.
Good categories:

- known page search
- site-restricted search
- technical documentation
- news article
- encyclopedia page
- dynamic page read
- full search plus read
