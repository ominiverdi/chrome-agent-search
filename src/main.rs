use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;
use wait_timeout::ChildExt;

mod parsers;
mod scoring;

use parsers::{parse_bing_html, parse_brave_html, parse_duckduckgo_lite_html};
use scoring::assess_quality;

#[derive(Parser, Debug, Clone)]
#[command(name = "chrome-agent-search")]
#[command(about = "Search and read web pages through chrome-agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, default_value = "chrome-agent", global = true)]
    chrome_agent: String,

    #[arg(long, default_value = "default", global = true)]
    browser: String,

    #[arg(long, global = true)]
    stealth: bool,

    #[arg(long, global = true)]
    copy_cookies: bool,

    #[arg(long, global = true)]
    headed: bool,

    #[arg(long, global = true)]
    connect: Option<String>,

    #[arg(long, default_value_t = 30, global = true)]
    chrome_timeout: u64,

    #[arg(long, default_value_t = 90, global = true)]
    command_timeout: u64,

    #[arg(long, default_value_t = 5, global = true)]
    wait_timeout: u64,

    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    Read {
        url: String,
        #[arg(long, default_value_t = 16000)]
        max_chars: usize,
    },
    Text {
        url: String,
        #[arg(long, default_value = "body")]
        selector: String,
        #[arg(long, default_value_t = 16000)]
        max_chars: usize,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 5)]
        limit: usize,
        #[arg(long, value_enum, default_value_t = Engine::Auto)]
        engine: Engine,
    },
    Full {
        query: String,
        #[arg(long, default_value_t = 5)]
        limit: usize,
        #[arg(long, default_value_t = 8000)]
        max_chars: usize,
        #[arg(long, default_value_t = 3)]
        concurrency: usize,
        #[arg(long)]
        read_timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = Engine::Auto)]
        engine: Engine,
    },
    Bench {
        #[arg(long, default_value = "benchmarks/cases.toml")]
        cases: String,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, default_value_t = 3)]
        concurrency: usize,
        #[arg(long, value_enum, default_value_t = Engine::Auto)]
        engine: Engine,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Engine {
    Auto,
    All,
    Bing,
    Brave,
    Duckduckgo,
}

#[derive(Serialize)]
struct TimedText {
    ok: bool,
    url: String,
    page: String,
    goto_seconds: f64,
    read_seconds: f64,
    text_chars: usize,
    text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
}

impl SearchResult {
    fn new(title: String, url: String, snippet: String) -> Self {
        Self {
            title,
            url,
            snippet,
            sources: Vec::new(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
struct SearchOutput {
    ok: bool,
    query: String,
    engine: String,
    page: String,
    seconds: f64,
    count: usize,
    results: Vec<SearchResult>,
}

#[derive(Serialize)]
struct FullResult {
    title: String,
    url: String,
    snippet: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
    fetch_status: String,
    content: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct FullOutput {
    ok: bool,
    query: String,
    engine: String,
    count: usize,
    results: Vec<FullResult>,
}

#[derive(Deserialize)]
struct BenchCases {
    cases: Vec<BenchCase>,
}

#[derive(Deserialize)]
struct BenchCase {
    id: String,
    kind: BenchKind,
    query: Option<String>,
    url: Option<String>,
    limit: Option<usize>,
    max_chars: Option<usize>,
    expected_urls: Option<Vec<String>>,
    expected_domains: Option<Vec<String>>,
    keywords: Option<Vec<String>>,
    min_chars: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum BenchKind {
    Search,
    Read,
    Full,
}

#[derive(Serialize)]
struct BenchRecord {
    case_id: String,
    kind: String,
    ok: bool,
    engine: Option<String>,
    latency_seconds: f64,
    result_count: Option<usize>,
    has_expected_url: bool,
    expected_url_rank: Option<usize>,
    expected_domain_rank: Option<usize>,
    keyword_coverage: f64,
    chars: Option<usize>,
    error: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Read { url, max_chars } => {
            let out = read_url(&cli, url, *max_chars)?;
            print_read(&out, cli.json)?;
        }
        Commands::Text {
            url,
            selector,
            max_chars,
        } => {
            let out = text_url(&cli, url, selector, *max_chars)?;
            print_read(&out, cli.json)?;
        }
        Commands::Search {
            query,
            limit,
            engine,
        } => {
            let out = search(&cli, query, *limit, *engine)?;
            print_search(&out, cli.json)?;
        }
        Commands::Full {
            query,
            limit,
            max_chars,
            concurrency,
            read_timeout,
            engine,
        } => {
            let out = full_search(
                &cli,
                query,
                *limit,
                *max_chars,
                *engine,
                *concurrency,
                *read_timeout,
            )?;
            print_full(&out, cli.json)?;
        }
        Commands::Bench {
            cases,
            out,
            concurrency,
            engine,
        } => {
            run_bench(&cli, cases, out.as_deref(), *engine, *concurrency)?;
        }
    }
    Ok(())
}

fn chrome_base(cli: &Cli, page: &str) -> Command {
    chrome_base_inner(cli, page, cli.stealth)
}

fn chrome_base_stealth(cli: &Cli, page: &str) -> Command {
    chrome_base_inner(cli, page, true)
}

fn chrome_base_inner(cli: &Cli, page: &str, stealth: bool) -> Command {
    let mut cmd = Command::new(&cli.chrome_agent);
    cmd.arg("--browser")
        .arg(&cli.browser)
        .arg("--page")
        .arg(page)
        .arg("--timeout")
        .arg(cli.chrome_timeout.to_string());
    if stealth {
        cmd.arg("--stealth");
    }
    if cli.copy_cookies {
        cmd.arg("--copy-cookies");
    }
    if cli.headed {
        cmd.arg("--headed");
    }
    if let Some(connect) = &cli.connect {
        cmd.arg("--connect").arg(connect);
    }
    cmd
}

fn run_cmd(mut cmd: Command, timeout_secs: u64) -> Result<String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stdout_path = std::env::temp_dir().join(format!(
        "chrome-agent-search-{}-{stamp}.stdout",
        std::process::id()
    ));
    let stderr_path = std::env::temp_dir().join(format!(
        "chrome-agent-search-{}-{stamp}.stderr",
        std::process::id()
    ));
    let stdout_file = File::create(&stdout_path).context("create stdout temp file")?;
    let stderr_file = File::create(&stderr_path).context("create stderr temp file")?;
    let mut child = cmd
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .context("failed to run chrome-agent")?;
    let status = match child
        .wait_timeout(Duration::from_secs(timeout_secs))
        .context("failed waiting for chrome-agent")?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(anyhow!("chrome-agent timed out after {timeout_secs}s"));
        }
    };
    let out = fs::read_to_string(&stdout_path).unwrap_or_default();
    let err = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    if !status.success() {
        return Err(anyhow!("chrome-agent failed: {}{}", err, out));
    }
    Ok(out.trim().to_string())
}

fn page_name(prefix: &str, value: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(value.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    format!("cas-{}-{}", prefix, &hash[..12])
}

fn goto(cli: &Cli, page: &str, url: &str) -> Result<(String, f64)> {
    let start = Instant::now();
    let mut cmd = chrome_base(cli, page);
    cmd.arg("goto").arg(url);
    let text = run_cmd(cmd, cli.command_timeout)?;
    Ok((text, start.elapsed().as_secs_f64()))
}

fn page_html(cli: &Cli, page: &str, stealth: bool) -> Result<String> {
    let mut cmd = if stealth {
        chrome_base_stealth(cli, page)
    } else {
        chrome_base(cli, page)
    };
    cmd.arg("eval").arg("document.documentElement.outerHTML");
    let raw = run_cmd(cmd, cli.command_timeout)?;
    Ok(serde_json::from_str::<String>(&raw).unwrap_or(raw))
}

fn read_url(cli: &Cli, url: &str, max_chars: usize) -> Result<TimedText> {
    let page = page_name("read", url);
    let (_goto_text, goto_seconds) = goto(cli, &page, url)?;
    let start = Instant::now();
    let mut cmd = chrome_base(cli, &page);
    cmd.arg("read").arg("--truncate").arg(max_chars.to_string());
    let text = run_cmd(cmd, cli.command_timeout)?;
    Ok(TimedText {
        ok: true,
        url: url.to_string(),
        page,
        goto_seconds,
        read_seconds: start.elapsed().as_secs_f64(),
        text_chars: text.len(),
        text,
    })
}

fn text_url(cli: &Cli, url: &str, selector: &str, max_chars: usize) -> Result<TimedText> {
    let page = page_name("text", url);
    let (_goto_text, goto_seconds) = goto(cli, &page, url)?;
    let start = Instant::now();
    let mut cmd = chrome_base(cli, &page);
    cmd.arg("text")
        .arg("--selector")
        .arg(selector)
        .arg("--truncate")
        .arg(max_chars.to_string());
    let text = run_cmd(cmd, cli.command_timeout)?;
    Ok(TimedText {
        ok: true,
        url: url.to_string(),
        page,
        goto_seconds,
        read_seconds: start.elapsed().as_secs_f64(),
        text_chars: text.len(),
        text,
    })
}

fn full_search(
    cli: &Cli,
    query: &str,
    limit: usize,
    max_chars: usize,
    engine: Engine,
    concurrency: usize,
    read_timeout: Option<u64>,
) -> Result<FullOutput> {
    let search_out = search(cli, query, limit, engine)?;
    let items: Vec<SearchResult> = search_out.results.into_iter().take(limit).collect();
    let results =
        read_results_concurrently(cli, items, max_chars, concurrency.max(1), read_timeout);
    Ok(FullOutput {
        ok: true,
        query: query.to_string(),
        engine: search_out.engine,
        count: results.len(),
        results,
    })
}

fn read_results_concurrently(
    cli: &Cli,
    items: Vec<SearchResult>,
    max_chars: usize,
    concurrency: usize,
    read_timeout: Option<u64>,
) -> Vec<FullResult> {
    let mut out = Vec::with_capacity(items.len());
    for chunk in items.chunks(concurrency) {
        let mut handles = Vec::with_capacity(chunk.len());
        for item in chunk.iter().cloned() {
            let mut cli = cli.clone();
            if let Some(read_timeout) = read_timeout {
                cli.command_timeout = read_timeout;
                cli.chrome_timeout = read_timeout;
            }
            handles.push(thread::spawn(move || {
                match read_url(&cli, &item.url, max_chars) {
                    Ok(read) => FullResult {
                        title: item.title,
                        url: item.url,
                        snippet: item.snippet,
                        sources: item.sources,
                        fetch_status: "success".to_string(),
                        content: Some(read.text),
                        error: None,
                    },
                    Err(err) => FullResult {
                        title: item.title,
                        url: item.url,
                        snippet: item.snippet,
                        sources: item.sources,
                        fetch_status: "error".to_string(),
                        content: None,
                        error: Some(err.to_string()),
                    },
                }
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(result) => out.push(result),
                Err(_) => out.push(FullResult {
                    title: "thread panic".to_string(),
                    url: String::new(),
                    snippet: String::new(),
                    sources: Vec::new(),
                    fetch_status: "error".to_string(),
                    content: None,
                    error: Some("reader thread panicked".to_string()),
                }),
            }
        }
    }
    out
}

fn search(cli: &Cli, query: &str, limit: usize, engine: Engine) -> Result<SearchOutput> {
    match engine {
        Engine::Bing => search_bing(cli, query, limit),
        Engine::Brave => search_brave(cli, query, limit),
        Engine::Duckduckgo => search_duckduckgo(cli, query, limit),
        Engine::Auto => search_auto(cli, query, limit),
        Engine::All => search_all(cli, query, limit),
    }
}

fn search_auto(cli: &Cli, query: &str, limit: usize) -> Result<SearchOutput> {
    let start = Instant::now();
    let fast = run_searches_parallel(cli, query, limit, &[search_brave, search_duckduckgo]);
    if fast.is_empty() {
        return search_all(cli, query, limit);
    }
    let fast_pages = fast.iter().map(|out| out.page.clone()).collect::<Vec<_>>();
    let fast_candidates = fast
        .into_iter()
        .flat_map(|out| out.results.into_iter())
        .collect::<Vec<_>>();
    let fast_results = merge_results(fast_candidates, query, limit);
    let top_quality = fast_results
        .first()
        .map(|result| assess_quality(std::slice::from_ref(result), query))
        .unwrap_or(0.0);
    if top_quality >= 0.75 || assess_quality(&fast_results, query) >= 0.65 {
        return Ok(SearchOutput {
            ok: true,
            query: query.to_string(),
            engine: "auto".to_string(),
            page: fast_pages.join(","),
            seconds: start.elapsed().as_secs_f64(),
            count: fast_results.len(),
            results: fast_results,
        });
    }

    let mut pages = fast_pages;
    let mut candidates = fast_results;
    if let Ok(out) = search_bing(cli, query, limit) {
        pages.push(out.page);
        candidates.extend(out.results);
    }
    if candidates.is_empty() {
        return Err(anyhow!("all search engines failed"));
    }
    let results = merge_results(candidates, query, limit);
    Ok(SearchOutput {
        ok: true,
        query: query.to_string(),
        engine: "auto".to_string(),
        page: pages.join(","),
        seconds: start.elapsed().as_secs_f64(),
        count: results.len(),
        results,
    })
}

fn search_all(cli: &Cli, query: &str, limit: usize) -> Result<SearchOutput> {
    let start = Instant::now();
    let outputs = run_searches_parallel(
        cli,
        query,
        limit,
        &[search_bing, search_brave, search_duckduckgo],
    );
    let mut candidates = Vec::new();
    let mut pages = Vec::new();
    for out in outputs {
        pages.push(out.page);
        candidates.extend(out.results);
    }
    if candidates.is_empty() {
        return Err(anyhow!("all search engines failed"));
    }
    let results = merge_results(candidates, query, limit);
    Ok(SearchOutput {
        ok: true,
        query: query.to_string(),
        engine: "all".to_string(),
        page: pages.join(","),
        seconds: start.elapsed().as_secs_f64(),
        count: results.len(),
        results,
    })
}

fn run_searches_parallel(
    cli: &Cli,
    query: &str,
    limit: usize,
    searches: &[fn(&Cli, &str, usize) -> Result<SearchOutput>],
) -> Vec<SearchOutput> {
    let handles = searches
        .iter()
        .copied()
        .map(|search_fn| {
            let cli = cli.clone();
            let query = query.to_string();
            thread::spawn(move || search_fn(&cli, &query, limit))
        })
        .collect::<Vec<_>>();
    handles
        .into_iter()
        .filter_map(|handle| match handle.join() {
            Ok(Ok(out)) => Some(out),
            _ => None,
        })
        .collect()
}

fn tag_sources(results: &mut [SearchResult], source: &str) {
    for result in results {
        if !result.sources.iter().any(|existing| existing == source) {
            result.sources.push(source.to_string());
        }
    }
}

fn merge_results(results: Vec<SearchResult>, query: &str, limit: usize) -> Vec<SearchResult> {
    let mut merged: HashMap<String, (SearchResult, f64)> = HashMap::new();
    let mut source_ranks: HashMap<String, usize> = HashMap::new();
    for mut result in results {
        let source = result
            .sources
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let rank = source_ranks.entry(source).or_insert(0);
        *rank += 1;
        let key = canonical_url(&result.url);
        let rank_weight = 1.0 / *rank as f64;
        let entry = merged.entry(key).or_insert_with(|| (result.clone(), 0.0));
        entry.1 += rank_weight;
        for source in result.sources.drain(..) {
            if !entry.0.sources.contains(&source) {
                entry.0.sources.push(source);
            }
        }
        if result.title.len() > entry.0.title.len() {
            entry.0.title = result.title;
        }
        if result.snippet.len() > entry.0.snippet.len() {
            entry.0.snippet = result.snippet;
        }
    }
    let mut scored: Vec<(SearchResult, f64)> = merged
        .into_values()
        .map(|(result, agreement)| {
            let quality = assess_quality(std::slice::from_ref(&result), query);
            let source_bonus = (result.sources.len().saturating_sub(1) as f64) * 0.15;
            let score = quality + source_bonus + agreement.min(1.0) * 0.1;
            (result, score)
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored
        .into_iter()
        .map(|(result, _)| result)
        .take(limit)
        .collect()
}

fn canonical_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.trim_end_matches('/').to_lowercase();
    };
    url.set_fragment(None);
    let tracking_prefixes = ["utm_", "fbclid", "gclid", "mc_cid", "mc_eid", "igshid"];
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| {
            !tracking_prefixes
                .iter()
                .any(|prefix| key.starts_with(prefix))
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    if !kept.is_empty() {
        let query = kept
            .into_iter()
            .map(|(key, value)| format!("{}={}", key, value))
            .collect::<Vec<_>>()
            .join("&");
        url.set_query(Some(&query));
    }
    let mut out = url.to_string();
    if out.ends_with('/') {
        out.pop();
    }
    out.to_lowercase()
}

fn search_duckduckgo(cli: &Cli, query: &str, limit: usize) -> Result<SearchOutput> {
    let page = page_name("duckduckgo", query);
    let url = format!(
        "https://lite.duckduckgo.com/lite/?q={}",
        urlencoding::encode(query)
    );
    let start = Instant::now();
    let mut goto_cmd = chrome_base_stealth(cli, &page);
    goto_cmd.arg("goto").arg(&url);
    run_cmd(goto_cmd, cli.command_timeout)?;
    let mut wait_cmd = chrome_base_stealth(cli, &page);
    wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("a.result-link, .result__title a")
        .arg("--timeout")
        .arg(cli.wait_timeout.to_string());
    let _ = run_cmd(wait_cmd, cli.command_timeout);
    let html = page_html(cli, &page, true)?;
    let mut results = parse_duckduckgo_lite_html(&html, limit);
    tag_sources(&mut results, "duckduckgo");
    Ok(SearchOutput {
        ok: true,
        query: query.to_string(),
        engine: "duckduckgo".to_string(),
        page,
        seconds: start.elapsed().as_secs_f64(),
        count: results.len(),
        results,
    })
}

fn search_brave(cli: &Cli, query: &str, limit: usize) -> Result<SearchOutput> {
    let page = page_name("brave", query);
    let url = format!(
        "https://search.brave.com/search?q={}&source=web",
        urlencoding::encode(query)
    );
    let start = Instant::now();
    let mut goto_cmd = chrome_base_stealth(cli, &page);
    goto_cmd.arg("goto").arg(&url);
    run_cmd(goto_cmd, cli.command_timeout)?;
    let mut wait_cmd = chrome_base_stealth(cli, &page);
    wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("[data-type=web]")
        .arg("--timeout")
        .arg(cli.wait_timeout.to_string());
    let _ = run_cmd(wait_cmd, cli.command_timeout);
    let html = page_html(cli, &page, true)?;
    let mut results = parse_brave_html(&html, limit);
    tag_sources(&mut results, "brave");
    Ok(SearchOutput {
        ok: true,
        query: query.to_string(),
        engine: "brave".to_string(),
        page,
        seconds: start.elapsed().as_secs_f64(),
        count: results.len(),
        results,
    })
}

fn search_bing(cli: &Cli, query: &str, limit: usize) -> Result<SearchOutput> {
    let start = Instant::now();
    let homepage = search_bing_homepage(cli, query, limit).ok();
    let (page, results) = match homepage {
        Some((page, results)) if assess_quality(&results, query) >= 0.8 => (page, results),
        Some((home_page, home_results)) => match search_bing_direct(cli, query, limit).ok() {
            Some((direct_page, direct_results))
                if assess_quality(&direct_results, query)
                    > assess_quality(&home_results, query) =>
            {
                (direct_page, direct_results)
            }
            _ => (home_page, home_results),
        },
        None => search_bing_direct(cli, query, limit)?,
    };
    Ok(SearchOutput {
        ok: true,
        query: query.to_string(),
        engine: "bing".to_string(),
        page,
        seconds: start.elapsed().as_secs_f64(),
        count: results.len(),
        results,
    })
}

fn search_bing_homepage(
    cli: &Cli,
    query: &str,
    limit: usize,
) -> Result<(String, Vec<SearchResult>)> {
    let page = page_name("bing-home", query);
    goto(cli, &page, "https://www.bing.com")?;
    let mut wait_cmd = chrome_base(cli, &page);
    wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("#sb_form_q")
        .arg("--timeout")
        .arg(cli.wait_timeout.to_string());
    run_cmd(wait_cmd, cli.command_timeout)?;
    let mut fill_cmd = chrome_base(cli, &page);
    fill_cmd
        .arg("fill")
        .arg("--selector")
        .arg("#sb_form_q")
        .arg(query);
    run_cmd(fill_cmd, cli.command_timeout)?;
    let mut press_cmd = chrome_base(cli, &page);
    press_cmd.arg("press").arg("Enter");
    run_cmd(press_cmd, cli.command_timeout)?;
    let mut results_wait_cmd = chrome_base(cli, &page);
    results_wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("li.b_algo")
        .arg("--timeout")
        .arg(cli.wait_timeout.to_string());
    let _ = run_cmd(results_wait_cmd, cli.command_timeout);
    let html = page_html(cli, &page, false)?;
    let mut results = parse_bing_html(&html, limit);
    if results.is_empty() {
        results = parse_bing_text_fallback(cli, &page, limit)?;
    }
    tag_sources(&mut results, "bing");
    Ok((page, results))
}

fn search_bing_direct(cli: &Cli, query: &str, limit: usize) -> Result<(String, Vec<SearchResult>)> {
    let page = page_name("search", query);
    let url = format!(
        "https://www.bing.com/search?q={}&count={}&form=QBLH&sp=-1&qs=n&cvid={}",
        urlencoding::encode(query),
        limit.max(10),
        bing_cvid(query)
    );
    goto(cli, &page, &url)?;
    let mut wait_cmd = chrome_base(cli, &page);
    wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("li.b_algo")
        .arg("--timeout")
        .arg(cli.wait_timeout.to_string());
    let _ = run_cmd(wait_cmd, cli.command_timeout);
    let html = page_html(cli, &page, false)?;
    let mut results = parse_bing_html(&html, limit);
    if results.is_empty() {
        results = parse_bing_text_fallback(cli, &page, limit)?;
    }
    tag_sources(&mut results, "bing");
    Ok((page, results))
}

fn bing_cvid(query: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(query.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    hash[..32].to_string()
}

fn parse_bing_text_fallback(cli: &Cli, page: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let mut cmd = chrome_base(cli, page);
    cmd.arg("text")
        .arg("--selector")
        .arg("body")
        .arg("--truncate")
        .arg("12000");
    let text = run_cmd(cmd, cli.command_timeout)?;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < lines.len() && out.len() < limit {
        if lines[i].contains('.') && lines[i + 1].starts_with("http") {
            let title = lines.get(i + 2).unwrap_or(&"").to_string();
            let url = lines[i + 1].replace(" › ", "/").replace(' ', "");
            let snippet = lines.get(i + 3).unwrap_or(&"").to_string();
            if !title.is_empty() && !url.contains("bing.com") {
                out.push(SearchResult::new(title, url, snippet));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    Ok(out)
}

fn run_bench(
    cli: &Cli,
    cases_path: &str,
    out_path: Option<&str>,
    engine: Engine,
    concurrency: usize,
) -> Result<()> {
    let text = fs::read_to_string(cases_path).with_context(|| format!("read {cases_path}"))?;
    let cases: BenchCases = toml::from_str(&text).with_context(|| format!("parse {cases_path}"))?;
    let mut records = Vec::new();
    for case in &cases.cases {
        let record = run_bench_case(cli, case, engine, concurrency);
        if !cli.json {
            println!(
                "{} {} ok={} latency={:.3}s coverage={:.2}",
                record.case_id,
                record.kind,
                record.ok,
                record.latency_seconds,
                record.keyword_coverage
            );
        }
        records.push(record);
    }
    let jsonl = records
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n")
        + "\n";
    if let Some(path) = out_path {
        fs::write(path, jsonl).with_context(|| format!("write {path}"))?;
    } else if cli.json {
        print!("{jsonl}");
    }
    if !cli.json {
        print_bench_summary(&records);
    }
    Ok(())
}

fn run_bench_case(cli: &Cli, case: &BenchCase, engine: Engine, concurrency: usize) -> BenchRecord {
    let start = Instant::now();
    match case.kind {
        BenchKind::Search => {
            let query = match &case.query {
                Some(q) => q,
                None => {
                    return bench_error(
                        case,
                        "search case missing query",
                        start.elapsed().as_secs_f64(),
                    );
                }
            };
            match search(cli, query, case.limit.unwrap_or(5), engine) {
                Ok(out) => {
                    let haystacks: Vec<String> =
                        out.results.iter().map(search_result_text).collect();
                    BenchRecord {
                        case_id: case.id.clone(),
                        kind: "search".to_string(),
                        ok: bench_ok(
                            &case.expected_urls,
                            &case.expected_domains,
                            &case.keywords,
                            &haystacks,
                            None,
                        ),
                        engine: Some(out.engine),
                        latency_seconds: start.elapsed().as_secs_f64(),
                        result_count: Some(out.results.len()),
                        has_expected_url: case
                            .expected_urls
                            .as_ref()
                            .map(|v| !v.is_empty())
                            .unwrap_or(false),
                        expected_url_rank: rank_url(&out.results, case.expected_urls.as_deref()),
                        expected_domain_rank: rank_domain(
                            &out.results,
                            case.expected_domains.as_deref(),
                        ),
                        keyword_coverage: keyword_coverage(
                            &haystacks.join("\n"),
                            case.keywords.as_deref(),
                        ),
                        chars: None,
                        error: None,
                    }
                }
                Err(err) => bench_error(case, &err.to_string(), start.elapsed().as_secs_f64()),
            }
        }
        BenchKind::Read => {
            let url = match &case.url {
                Some(u) => u,
                None => {
                    return bench_error(
                        case,
                        "read case missing url",
                        start.elapsed().as_secs_f64(),
                    );
                }
            };
            match read_url(cli, url, case.max_chars.unwrap_or(16000)) {
                Ok(out) => {
                    let chars = out.text.len();
                    let min_ok = case.min_chars.map(|min| chars >= min).unwrap_or(true);
                    let coverage = keyword_coverage(&out.text, case.keywords.as_deref());
                    BenchRecord {
                        case_id: case.id.clone(),
                        kind: "read".to_string(),
                        ok: min_ok && coverage >= required_coverage(case.keywords.as_deref()),
                        engine: None,
                        latency_seconds: start.elapsed().as_secs_f64(),
                        result_count: None,
                        has_expected_url: false,
                        expected_url_rank: None,
                        expected_domain_rank: None,
                        keyword_coverage: coverage,
                        chars: Some(chars),
                        error: None,
                    }
                }
                Err(err) => bench_error(case, &err.to_string(), start.elapsed().as_secs_f64()),
            }
        }
        BenchKind::Full => {
            let query = match &case.query {
                Some(q) => q,
                None => {
                    return bench_error(
                        case,
                        "full case missing query",
                        start.elapsed().as_secs_f64(),
                    );
                }
            };
            match full_search(
                cli,
                query,
                case.limit.unwrap_or(3),
                case.max_chars.unwrap_or(8000),
                engine,
                concurrency,
                None,
            ) {
                Ok(out) => {
                    let mut combined = String::new();
                    let mut chars = 0;
                    for result in &out.results {
                        if let Some(content) = &result.content {
                            chars += content.len();
                            combined.push_str(content);
                            combined.push('\n');
                        }
                    }
                    let haystacks: Vec<String> = out.results.iter().map(full_result_text).collect();
                    let coverage = keyword_coverage(&combined, case.keywords.as_deref());
                    BenchRecord {
                        case_id: case.id.clone(),
                        kind: "full".to_string(),
                        ok: bench_ok(
                            &case.expected_urls,
                            &case.expected_domains,
                            &case.keywords,
                            &haystacks,
                            Some(&combined),
                        ),
                        engine: Some(out.engine),
                        latency_seconds: start.elapsed().as_secs_f64(),
                        result_count: Some(out.results.len()),
                        has_expected_url: case
                            .expected_urls
                            .as_ref()
                            .map(|v| !v.is_empty())
                            .unwrap_or(false),
                        expected_url_rank: rank_url_full(
                            &out.results,
                            case.expected_urls.as_deref(),
                        ),
                        expected_domain_rank: rank_domain_full(
                            &out.results,
                            case.expected_domains.as_deref(),
                        ),
                        keyword_coverage: coverage,
                        chars: Some(chars),
                        error: None,
                    }
                }
                Err(err) => bench_error(case, &err.to_string(), start.elapsed().as_secs_f64()),
            }
        }
    }
}

fn bench_error(case: &BenchCase, error: &str, latency_seconds: f64) -> BenchRecord {
    BenchRecord {
        case_id: case.id.clone(),
        kind: match case.kind {
            BenchKind::Search => "search",
            BenchKind::Read => "read",
            BenchKind::Full => "full",
        }
        .to_string(),
        ok: false,
        engine: None,
        latency_seconds,
        result_count: None,
        has_expected_url: case
            .expected_urls
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        expected_url_rank: None,
        expected_domain_rank: None,
        keyword_coverage: 0.0,
        chars: None,
        error: Some(error.to_string()),
    }
}

fn bench_ok(
    expected_urls: &Option<Vec<String>>,
    expected_domains: &Option<Vec<String>>,
    keywords: &Option<Vec<String>>,
    search_texts: &[String],
    content: Option<&str>,
) -> bool {
    let url_ok = expected_urls
        .as_ref()
        .map(|urls| {
            search_texts
                .iter()
                .any(|text| urls.iter().any(|url| text.contains(url)))
        })
        .unwrap_or(true);
    let domain_ok = expected_domains
        .as_ref()
        .map(|domains| {
            search_texts
                .iter()
                .any(|text| domains.iter().any(|domain| text.contains(domain)))
        })
        .unwrap_or(true);
    let keyword_text = content.unwrap_or(&search_texts.join("\n")).to_string();
    let keywords_ok = keyword_coverage(&keyword_text, keywords.as_deref())
        >= required_coverage(keywords.as_deref());
    url_ok && domain_ok && keywords_ok
}

fn required_coverage(keywords: Option<&[String]>) -> f64 {
    if keywords.map(|k| k.is_empty()).unwrap_or(true) {
        0.0
    } else {
        0.6
    }
}

fn search_result_text(result: &SearchResult) -> String {
    format!("{} {} {}", result.title, result.url, result.snippet)
}

fn full_result_text(result: &FullResult) -> String {
    format!("{} {} {}", result.title, result.url, result.snippet)
}

fn keyword_coverage(text: &str, keywords: Option<&[String]>) -> f64 {
    let Some(keywords) = keywords else {
        return 1.0;
    };
    if keywords.is_empty() {
        return 1.0;
    }
    let lower = text.to_lowercase();
    let hits = keywords
        .iter()
        .filter(|k| lower.contains(&k.to_lowercase()))
        .count();
    hits as f64 / keywords.len() as f64
}

fn rank_url(results: &[SearchResult], expected: Option<&[String]>) -> Option<usize> {
    let expected = expected?;
    results
        .iter()
        .position(|r| expected.iter().any(|u| urls_match(&r.url, u)))
        .map(|idx| idx + 1)
}

fn rank_domain(results: &[SearchResult], expected: Option<&[String]>) -> Option<usize> {
    let expected = expected?;
    results
        .iter()
        .position(|r| expected.iter().any(|d| r.url.contains(d)))
        .map(|idx| idx + 1)
}

fn rank_url_full(results: &[FullResult], expected: Option<&[String]>) -> Option<usize> {
    let expected = expected?;
    results
        .iter()
        .position(|r| expected.iter().any(|u| urls_match(&r.url, u)))
        .map(|idx| idx + 1)
}

fn rank_domain_full(results: &[FullResult], expected: Option<&[String]>) -> Option<usize> {
    let expected = expected?;
    results
        .iter()
        .position(|r| expected.iter().any(|d| r.url.contains(d)))
        .map(|idx| idx + 1)
}

fn urls_match(actual: &str, expected: &str) -> bool {
    actual.trim_end_matches('/') == expected.trim_end_matches('/')
}

fn print_bench_summary(records: &[BenchRecord]) {
    let total = records.len();
    let pass = records.iter().filter(|r| r.ok).count();
    let avg = if total == 0 {
        0.0
    } else {
        records.iter().map(|r| r.latency_seconds).sum::<f64>() / total as f64
    };
    let search_with_url = records
        .iter()
        .filter(|r| r.kind == "search" && r.has_expected_url)
        .count();
    let url_at_1 = records
        .iter()
        .filter(|r| r.kind == "search" && r.expected_url_rank == Some(1))
        .count();
    let url_at_5 = records
        .iter()
        .filter(|r| {
            r.kind == "search" && r.expected_url_rank.map(|rank| rank <= 5).unwrap_or(false)
        })
        .count();
    println!("\nSummary");
    println!("Cases: {total}");
    println!("Pass: {pass}/{total}");
    println!("Search URL@1: {url_at_1}/{search_with_url}");
    println!("Search URL@5: {url_at_5}/{search_with_url}");
    println!("Avg latency: {avg:.3}s");
}

fn print_read(out: &TimedText, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(out)?);
    } else {
        println!("{}", out.text);
    }
    Ok(())
}

fn print_full(out: &FullOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(out)?);
    } else {
        println!(
            "Search completed for {:?} with {} results\n",
            out.query, out.count
        );
        for (idx, result) in out.results.iter().enumerate() {
            println!(
                "{}. {}\nURL: {}\nDescription: {}",
                idx + 1,
                result.title,
                result.url,
                result.snippet
            );
            match (&result.content, &result.error) {
                (Some(content), _) => println!("\nFull Content:\n{}\n---\n", content),
                (_, Some(error)) => println!("\nContent Extraction Failed: {}\n---\n", error),
                _ => println!("---\n"),
            }
        }
    }
    Ok(())
}

fn print_search(out: &SearchOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(out)?);
    } else {
        println!(
            "Search summaries for {:?} with {} results:\n",
            out.query, out.count
        );
        for (idx, item) in out.results.iter().enumerate() {
            println!(
                "{}. {}\nURL: {}\nDescription: {}\n",
                idx + 1,
                item.title,
                item.url,
                item.snippet
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    #[test]
    fn duckduckgo_lite_parser_extracts_result_and_decodes_redirect() {
        let html = r#"
        <html><body><table>
          <tr><td>1.</td><td><a class="result-link" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.zdnet.com%2Farticle%2Fgreg%2Dkroah%2Dhartman%2Dlinux%2Dkernel%2Drust%2F&amp;rut=abc">'Rust makes coding fun again' - ZDNET</a></td></tr>
          <tr><td></td><td>At Open Source Summit India, Greg Kroah-Hartman said the Linux kernel is moving toward Rust.</td></tr>
          <tr><td></td><td>www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/</td></tr>
        </table></body></html>
        "#;
        let results = parse_duckduckgo_lite_html(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "'Rust makes coding fun again' - ZDNET");
        assert_eq!(
            results[0].url,
            "https://www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/"
        );
        assert!(results[0].snippet.contains("Greg Kroah-Hartman"));
    }

    #[test]
    fn bing_parser_prefers_title_link_and_decodes_redirect() {
        let target = "https://thenextweb.com/news/speed-not-selling-point-ai-agents-caution";
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(target.as_bytes());
        let html = format!(
            r#"
            <html><body><ol>
              <li class="b_algo">
                <div><a href="https://www.bing.com/ck/a?u=a1{}">thenextweb.com › news</a></div>
                <h2><a href="https://www.bing.com/ck/a?u=a1{}">Speed is no longer a selling point for AI agents, caution is - TNW</a></h2>
                <div class="b_caption"><p>AI agents need caution more than raw speed.</p></div>
              </li>
            </ol></body></html>
            "#,
            encoded, encoded
        );
        let results = parse_bing_html(&html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].title,
            "Speed is no longer a selling point for AI agents, caution is - TNW"
        );
        assert_eq!(results[0].url, target);
        assert!(results[0].snippet.contains("caution"));
    }

    #[test]
    fn brave_parser_extracts_title_snippet_and_url() {
        let html = r#"
        <html><body>
          <div data-type="web">
            <a href="https://www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/">
              <div class="site-name-content"><div>ZDNET</div><cite>zdnet.com › article</cite></div>
              <div class="title search-snippet-title">'Rust makes coding fun again': Why Linux is moving away from C, according to Greg Kroah-Hartman | ZDNET</div>
            </a>
            <div class="generic-snippet"><div class="content">The Linux kernel is moving toward Rust, Greg Kroah-Hartman said.</div></div>
          </div>
        </body></html>
        "#;
        let results = parse_brave_html(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].url,
            "https://www.zdnet.com/article/greg-kroah-hartman-linux-kernel-rust/"
        );
        assert!(
            results[0]
                .title
                .starts_with("'Rust makes coding fun again'")
        );
        assert!(results[0].snippet.contains("Linux kernel"));
    }

    #[test]
    fn merged_results_dedupe_sources_and_keep_best_snippet() {
        let results = vec![
            SearchResult {
                title: "Short".to_string(),
                url: "https://example.com/article?utm_source=x#section".to_string(),
                snippet: "short".to_string(),
                sources: vec!["bing".to_string()],
            },
            SearchResult {
                title: "Better title about Rust and Linux".to_string(),
                url: "https://example.com/article".to_string(),
                snippet: "A longer snippet about Rust in the Linux kernel".to_string(),
                sources: vec!["duckduckgo".to_string()],
            },
        ];
        let merged = merge_results(results, "Rust Linux", 5);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].url,
            "https://example.com/article?utm_source=x#section"
        );
        assert_eq!(merged[0].sources, vec!["bing", "duckduckgo"]);
        assert!(merged[0].snippet.contains("Linux kernel"));
    }

    #[test]
    fn quality_scoring_prefers_specific_results() {
        let good = vec![SearchResult::new(
            "Speed is no longer a selling point for AI agents, caution is - TNW".to_string(),
            "https://thenextweb.com/news/speed-not-selling-point-ai-agents-caution".to_string(),
            "AI agents need a cautious pause.".to_string(),
        )];
        let weak = vec![SearchResult::new(
            "AI news".to_string(),
            "https://example.com/ai".to_string(),
            "General technology news.".to_string(),
        )];
        let query = "site:thenextweb.com/news speed no longer selling point AI agents caution";
        assert!(assess_quality(&good, query) > assess_quality(&weak, query));
    }
}
