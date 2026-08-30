use anyhow::{Context, Result, anyhow};
use base64::Engine as Base64Engine;
use clap::{Parser, Subcommand, ValueEnum};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs;
use std::process::Command;
use std::time::Instant;
use url::Url;

#[derive(Parser, Debug)]
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
    json: bool,
}

#[derive(Subcommand, Debug)]
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
        #[arg(long, value_enum, default_value_t = Engine::Auto)]
        engine: Engine,
    },
    Bench {
        #[arg(long, default_value = "benchmarks/cases.toml")]
        cases: String,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, value_enum, default_value_t = Engine::Auto)]
        engine: Engine,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Engine {
    Auto,
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
            engine,
        } => {
            let search_out = search(&cli, query, *limit, *engine)?;
            let mut results = Vec::new();
            for item in search_out.results.into_iter().take(*limit) {
                match read_url(&cli, &item.url, *max_chars) {
                    Ok(read) => results.push(FullResult {
                        title: item.title,
                        url: item.url,
                        snippet: item.snippet,
                        fetch_status: "success".to_string(),
                        content: Some(read.text),
                        error: None,
                    }),
                    Err(err) => results.push(FullResult {
                        title: item.title,
                        url: item.url,
                        snippet: item.snippet,
                        fetch_status: "error".to_string(),
                        content: None,
                        error: Some(err.to_string()),
                    }),
                }
            }
            let out = FullOutput {
                ok: true,
                query: query.clone(),
                engine: format!("{:?}", engine).to_lowercase(),
                count: results.len(),
                results,
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&out)?);
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
                        (_, Some(error)) => {
                            println!("\nContent Extraction Failed: {}\n---\n", error)
                        }
                        _ => println!("---\n"),
                    }
                }
            }
        }
        Commands::Bench { cases, out, engine } => {
            run_bench(&cli, cases, out.as_deref(), *engine)?;
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
        .arg(page);
    if stealth {
        cmd.arg("--stealth");
    }
    cmd
}

fn run(mut cmd: Command) -> Result<String> {
    let output = cmd.output().context("failed to run chrome-agent")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow!("chrome-agent failed: {}{}", err, out));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
    let text = run(cmd)?;
    Ok((text, start.elapsed().as_secs_f64()))
}

fn page_html(cli: &Cli, page: &str, stealth: bool) -> Result<String> {
    let mut cmd = if stealth {
        chrome_base_stealth(cli, page)
    } else {
        chrome_base(cli, page)
    };
    cmd.arg("eval").arg("document.documentElement.outerHTML");
    let raw = run(cmd)?;
    Ok(serde_json::from_str::<String>(&raw).unwrap_or(raw))
}

fn read_url(cli: &Cli, url: &str, max_chars: usize) -> Result<TimedText> {
    let page = page_name("read", url);
    let (_goto_text, goto_seconds) = goto(cli, &page, url)?;
    let start = Instant::now();
    let mut cmd = chrome_base(cli, &page);
    cmd.arg("read").arg("--truncate").arg(max_chars.to_string());
    let text = run(cmd)?;
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
    let text = run(cmd)?;
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

fn search(cli: &Cli, query: &str, limit: usize, engine: Engine) -> Result<SearchOutput> {
    match engine {
        Engine::Bing => search_bing(cli, query, limit),
        Engine::Brave => search_brave(cli, query, limit),
        Engine::Duckduckgo => search_duckduckgo(cli, query, limit),
        Engine::Auto => {
            let mut candidates = Vec::new();
            if let Ok(out) = search_bing(cli, query, limit) {
                candidates.push(out);
            }
            if let Ok(out) = search_brave(cli, query, limit) {
                candidates.push(out);
            }
            if let Ok(out) = search_duckduckgo(cli, query, limit) {
                candidates.push(out);
            }
            candidates
                .into_iter()
                .max_by(|a, b| {
                    assess_quality(&a.results, query).total_cmp(&assess_quality(&b.results, query))
                })
                .ok_or_else(|| anyhow!("all search engines failed"))
        }
    }
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("valid selector")
}

fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn element_text(element: scraper::ElementRef<'_>) -> String {
    clean_text(&element.text().collect::<Vec<_>>().join(" "))
}

fn clean_duckduckgo_url(href: &str) -> String {
    let parsed = Url::parse(href)
        .or_else(|_| Url::parse("https://duckduckgo.com").and_then(|base| base.join(href)));
    parsed
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "uddg")
                .map(|(_, value)| value.into_owned())
        })
        .unwrap_or_else(|| href.to_string())
}

fn clean_bing_url(href: &str) -> String {
    let Ok(url) = Url::parse(href) else {
        return href.to_string();
    };
    if !url.host_str().unwrap_or_default().contains("bing.com") {
        return href.to_string();
    }
    let Some(mut encoded) = url
        .query_pairs()
        .find(|(key, _)| key == "u")
        .map(|(_, value)| value.into_owned())
    else {
        return href.to_string();
    };
    if let Some(stripped) = encoded.strip_prefix("a1") {
        encoded = stripped.to_string();
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(encoded.as_bytes()))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| href.to_string())
}

fn parse_duckduckgo_lite_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let link_sel = selector("a.result-link, .result__title a, a.result__a");
    let tr_sel = selector("tr");
    let rows: Vec<_> = doc.select(&tr_sel).collect();
    let mut results = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        let Some(a) = row.select(&link_sel).next() else {
            continue;
        };
        let title = element_text(a);
        let href = a.value().attr("href").unwrap_or_default();
        let url = clean_duckduckgo_url(href);
        if title.is_empty() || url.contains("duckduckgo.com") {
            continue;
        }
        let mut snippet = String::new();
        for next_row in rows.iter().skip(idx + 1).take(3) {
            let text = element_text(*next_row);
            if !text.is_empty() && !text.starts_with("http") && !text.contains("duckduckgo.com") {
                snippet = text;
                break;
            }
        }
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
        if results.len() >= limit {
            break;
        }
    }
    results
}

fn parse_bing_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let item_sel = selector("li.b_algo, .b_result, .b_card");
    let primary_link_sel = selector("h2 a, .b_title a");
    let fallback_link_sel = selector("a[data-seid], a[href]");
    let snippet_sel = selector(
        ".b_caption p, .b_snippet, .b_descript, .b_caption, .b_excerpt, p, .b_algo_content",
    );
    let mut results = Vec::new();
    for item in doc.select(&item_sel) {
        let Some(a) = item
            .select(&primary_link_sel)
            .find(|a| !element_text(*a).is_empty())
            .or_else(|| {
                item.select(&fallback_link_sel)
                    .find(|a| !element_text(*a).is_empty())
            })
        else {
            continue;
        };
        let title = element_text(a);
        let url = clean_bing_url(a.value().attr("href").unwrap_or_default());
        if title.is_empty() || url.contains("bing.com") {
            continue;
        }
        let snippet = item
            .select(&snippet_sel)
            .map(element_text)
            .find(|text| !text.is_empty())
            .unwrap_or_default();
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
        if results.len() >= limit {
            break;
        }
    }
    results
}

fn parse_brave_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let item_sel = selector("[data-type=web]");
    let link_sel = selector("a[href]");
    let title_sel = selector(".title, .search-snippet-title");
    let snippet_sel = selector(".generic-snippet .content, .snippet-content, .description, p");
    let mut results = Vec::new();
    for item in doc.select(&item_sel) {
        let lines: Vec<String> = element_text(item)
            .split('\n')
            .map(clean_text)
            .filter(|line| !line.is_empty())
            .collect();
        let Some(a) = item.select(&link_sel).find(|a| {
            a.value()
                .attr("href")
                .unwrap_or_default()
                .starts_with("http")
        }) else {
            continue;
        };
        let url = a.value().attr("href").unwrap_or_default().to_string();
        if url.contains("search.brave.com") {
            continue;
        }
        let link_text = element_text(a);
        let title = item
            .select(&title_sel)
            .map(element_text)
            .find(|text| !text.is_empty())
            .or_else(|| {
                lines
                    .iter()
                    .enumerate()
                    .find(|(idx, line)| *idx > 1 && line.len() > 8 && !line.contains(" › "))
                    .map(|(_, line)| line.clone())
            })
            .unwrap_or(link_text);
        let snippet = item
            .select(&snippet_sel)
            .map(element_text)
            .find(|text| !text.is_empty())
            .or_else(|| {
                lines
                    .iter()
                    .enumerate()
                    .find(|(idx, line)| *idx > 2 && **line != title && line.len() > 40)
                    .map(|(_, line)| line.clone())
            })
            .unwrap_or_default();
        if !title.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }
        if results.len() >= limit {
            break;
        }
    }
    results
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
    run(goto_cmd)?;
    let mut wait_cmd = chrome_base_stealth(cli, &page);
    wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("a.result-link, .result__title a")
        .arg("--timeout")
        .arg("5");
    let _ = run(wait_cmd);
    let html = page_html(cli, &page, true)?;
    let results = parse_duckduckgo_lite_html(&html, limit);
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
    run(goto_cmd)?;
    let mut wait_cmd = chrome_base_stealth(cli, &page);
    wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("[data-type=web]")
        .arg("--timeout")
        .arg("5");
    let _ = run(wait_cmd);
    let html = page_html(cli, &page, true)?;
    let results = parse_brave_html(&html, limit);
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
    let page = page_name("search", query);
    let url = format!(
        "https://www.bing.com/search?q={}",
        urlencoding::encode(query)
    );
    let start = Instant::now();
    goto(cli, &page, &url)?;
    let mut wait_cmd = chrome_base(cli, &page);
    wait_cmd
        .arg("wait")
        .arg("selector")
        .arg("li.b_algo")
        .arg("--timeout")
        .arg("5");
    let _ = run(wait_cmd);
    let html = page_html(cli, &page, false)?;
    let mut results = parse_bing_html(&html, limit);
    if results.is_empty() {
        results = parse_bing_text_fallback(cli, &page, limit)?;
    }
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

fn parse_bing_text_fallback(cli: &Cli, page: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let mut cmd = chrome_base(cli, page);
    cmd.arg("text")
        .arg("--selector")
        .arg("body")
        .arg("--truncate")
        .arg("12000");
    let text = run(cmd)?;
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
                out.push(SearchResult {
                    title,
                    url,
                    snippet,
                });
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    Ok(out)
}

fn assess_quality(results: &[SearchResult], query: &str) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    let common = [
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
        "is", "are", "was", "were", "be", "been", "have", "has", "had", "do", "does", "did",
        "will", "would", "could", "should", "may", "might", "must", "can", "site",
    ];
    let normalized_query = query
        .split_whitespace()
        .filter(|term| !term.to_lowercase().starts_with("site:"))
        .collect::<Vec<_>>()
        .join(" ");
    let words: Vec<String> = normalized_query
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() > 2 && !common.contains(w))
        .map(ToString::to_string)
        .collect();
    if words.is_empty() {
        return 0.5;
    }
    let mut total = 0.0;
    let mut total_weight = 0.0;
    for (idx, result) in results.iter().enumerate() {
        let combined = format!("{} {} {}", result.title, result.snippet, result.url).to_lowercase();
        let keyword_matches = words
            .iter()
            .filter(|w| combined.contains(w.as_str()))
            .count();
        let mut phrase_matches = 0;
        for pair in words.windows(2) {
            if combined.contains(&pair.join(" ")) {
                phrase_matches += 1;
            }
        }
        if words.len() >= 3 && combined.contains(&words[..3].join(" ")) {
            phrase_matches += 1;
        }
        let score = (keyword_matches as f64 / words.len() as f64) + (phrase_matches as f64 * 0.3);
        let top_result_boost = if idx == 0 { 0.2 } else { 0.0 };
        let weight = 1.0 / (idx + 1) as f64;
        total += (score + top_result_boost).min(1.0) * weight;
        total_weight += weight;
    }
    total / total_weight
}

fn run_bench(cli: &Cli, cases_path: &str, out_path: Option<&str>, engine: Engine) -> Result<()> {
    let text = fs::read_to_string(cases_path).with_context(|| format!("read {cases_path}"))?;
    let cases: BenchCases = toml::from_str(&text).with_context(|| format!("parse {cases_path}"))?;
    let mut records = Vec::new();
    for case in &cases.cases {
        let record = run_bench_case(cli, case, engine);
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

fn run_bench_case(cli: &Cli, case: &BenchCase, engine: Engine) -> BenchRecord {
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
            match search(cli, query, case.limit.unwrap_or(3), engine) {
                Ok(out) => {
                    let mut combined = String::new();
                    let mut chars = 0;
                    for result in &out.results {
                        if let Ok(read) = read_url(cli, &result.url, case.max_chars.unwrap_or(8000))
                        {
                            chars += read.text.len();
                            combined.push_str(&read.text);
                            combined.push('\n');
                        }
                    }
                    let haystacks: Vec<String> =
                        out.results.iter().map(search_result_text).collect();
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
                        expected_url_rank: rank_url(&out.results, case.expected_urls.as_deref()),
                        expected_domain_rank: rank_domain(
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
    fn quality_scoring_prefers_specific_results() {
        let good = vec![SearchResult {
            title: "Speed is no longer a selling point for AI agents, caution is - TNW".to_string(),
            url: "https://thenextweb.com/news/speed-not-selling-point-ai-agents-caution"
                .to_string(),
            snippet: "AI agents need a cautious pause.".to_string(),
        }];
        let weak = vec![SearchResult {
            title: "AI news".to_string(),
            url: "https://example.com/ai".to_string(),
            snippet: "General technology news.".to_string(),
        }];
        let query = "site:thenextweb.com/news speed no longer selling point AI agents caution";
        assert!(assess_quality(&good, query) > assess_quality(&weak, query));
    }
}
