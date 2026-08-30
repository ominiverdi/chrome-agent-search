use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::time::Instant;

use crate::chrome::read_url;
use crate::full::full_search;
use crate::search::search;
use crate::types::{FullResult, SearchResult};
use crate::{Cli, Engine};

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
pub(crate) struct BenchRecord {
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

pub(crate) fn run_bench(
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

pub(crate) fn print_bench_summary(records: &[BenchRecord]) {
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
