# Benchmarks

Benchmarks are included to make changes measurable, not to claim universal search quality.

Search engines and websites are variable. Compare tools on the same machine, network, browser state, and time window.

## What is tested

Default cases live in [`benchmarks/cases.toml`](benchmarks/cases.toml).

Current default suite:

- 9 search cases
- 8 read cases
- 2 full-search cases
- 19 total cases

Checks include:

- expected URL rank when a canonical URL is known
- expected domain rank when several sources are acceptable
- keyword coverage
- minimum extracted character count for read/full cases
- latency per case

Hard diagnostic cases live in [`benchmarks/hard-cases.toml`](benchmarks/hard-cases.toml). These are intentionally separate because some sites may block automation, require login, or return human-verification pages.

## Run chrome-agent-search benchmark

```sh
cargo build --release
./target/release/chrome-agent-search \
  --command-timeout 30 \
  --wait-timeout 3 \
  bench \
  --cases benchmarks/cases.toml \
  --out /tmp/chrome-agent-search-bench.jsonl
```

Latest local run:

| Tool | Cases | Pass | Search URL@1 | Search URL@5 | Avg latency |
|---|---:|---:|---:|---:|---:|
| chrome-agent-search | 19 | 19/19 | 6/6 | 6/6 | 1.161s |

## Same-environment comparison with web-search-mcp

This comparison used the same public benchmark cases, same machine, same network, and same time window.

Compared tools:

- `chrome-agent-search` CLI at commit `03f801e`
- local checkout of `mrkrsl/web-search-mcp` at `/home/ominiverdi/github/archive/web-search-mcp`

Comparison command was run from the local lab harness, not committed to this repo:

```sh
python3 chrome-agent-search-lab/scripts/compare_web_search_mcp.py \
  --cases /home/ominiverdi/github/chrome-agent-search/benchmarks/cases.toml \
  --cas-binary /home/ominiverdi/github/chrome-agent-search/target/release/chrome-agent-search \
  --mcp-dir /home/ominiverdi/github/archive/web-search-mcp \
  --out chrome-agent-search-lab/results/web-search-mcp-comparison-current.jsonl \
  --raw-dir chrome-agent-search-lab/results/raw-current
```

Latest comparison result:

| Tool | Cases | Pass | Avg latency |
|---|---:|---:|---:|
| chrome-agent-search | 19 | 19/19 | 1.375s |
| web-search-mcp | 19 | 14/19 | 6.178s |

Search cases:

| Case | chrome-agent-search | web-search-mcp |
|---|---:|---:|
| tnw-ai-agents-caution | URL@1, 1.214s | URL@1, 10.825s |
| linux-rust-greg-kh | domain@1, 1.077s | domain@1, 4.722s |
| cerebras-openai-inference | domain@1, 1.291s | domain@2, 8.245s |
| quantum-light-engine | pass, 1.294s | pass, 8.877s |
| mdn-array-map | URL@1, 1.344s | URL@1, 8.867s |
| rust-vec-docs | URL@1, 1.625s | URL@1, 9.273s |
| github-ripgrep | URL@1, 1.309s | URL@1, 8.579s |
| ietf-rfc-9110 | URL@1, 1.313s | URL@1, 5.826s |
| arxiv-attention-is-all-you-need | URL@1, 1.235s | URL@1, 6.644s |

Read/full cases:

| Case | chrome-agent-search | web-search-mcp |
|---|---:|---:|
| kernel-org-read | pass, 0.425s | fail, 0.338s |
| wikipedia-ai-read | pass, 0.844s | fail, 2.410s |
| mdn-array-map-read | pass, 0.489s | pass, 0.345s |
| rust-vec-docs-read | pass, 0.689s | pass, 0.528s |
| rfc-9110-read | pass, 1.536s | fail, 3.373s |
| hacker-news-frontpage-read | pass, 1.146s | pass, 1.198s |
| github-ripgrep-issue-read | pass, 2.177s | fail, 3.571s |
| stackoverflow-arrow-operator-read | pass, 3.005s | fail, 3.810s |
| tnw-full | URL@1, pass, 1.694s | URL@1, pass, 13.960s |
| mdn-array-map-full | URL@1, pass, 2.426s | URL@1, pass, 15.989s |

## Interpretation

In this local same-environment run:

- Both tools found the expected top search result for all URL-specific search cases.
- `chrome-agent-search` was faster on every search case in this run.
- `chrome-agent-search` passed all read/full cases in this run.
- `web-search-mcp` failed several read cases, usually by extracting too little content or failing extraction.

This is not a global benchmark. It is a reproducible local smoke benchmark and comparison harness.
