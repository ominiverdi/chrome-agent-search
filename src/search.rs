use anyhow::{Result, anyhow};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::thread;
use std::time::Instant;
use url::Url;

use crate::chrome::{chrome_base, chrome_base_stealth, goto, page_html, page_name, run_cmd};
use crate::parsers::{parse_bing_html, parse_brave_html, parse_duckduckgo_lite_html};
use crate::scoring::assess_quality;
use crate::types::{SearchOutput, SearchResult};
use crate::{Cli, Engine};

type SearchFn = fn(&Cli, &str, usize) -> Result<SearchOutput>;

pub(crate) fn search(cli: &Cli, query: &str, limit: usize, engine: Engine) -> Result<SearchOutput> {
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
    let fast = run_searches_parallel(
        cli,
        query,
        limit,
        &[("brave", search_brave), ("duckduckgo", search_duckduckgo)],
    );
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
        &[
            ("bing", search_bing),
            ("brave", search_brave),
            ("duckduckgo", search_duckduckgo),
        ],
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
    searches: &[(&str, SearchFn)],
) -> Vec<SearchOutput> {
    let handles = searches
        .iter()
        .copied()
        .map(|(engine_name, search_fn)| {
            let mut cli = cli.clone();
            if cli.connect.is_none() {
                cli.browser = format!("{}-{}", cli.browser, engine_name);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SearchResult;

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
}
