use std::thread;

use anyhow::Result;

use crate::chrome::{goto, page_name, read_page, text_page};
use crate::search::search;
use crate::types::{FullOutput, FullResult, SearchResult};
use crate::{Cli, Engine};

pub(crate) fn full_search(
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

#[derive(Clone)]
struct PreparedResult {
    item: SearchResult,
    page: String,
    goto_seconds: f64,
    navigation_error: Option<String>,
}

fn prepare_results(cli: &Cli, items: Vec<SearchResult>) -> Vec<PreparedResult> {
    items
        .into_iter()
        .map(|item| {
            let page = page_name("read", &item.url);
            match goto(cli, &page, &item.url) {
                Ok((_output, goto_seconds)) => PreparedResult {
                    item,
                    page,
                    goto_seconds,
                    navigation_error: None,
                },
                Err(err) => PreparedResult {
                    item,
                    page,
                    goto_seconds: 0.0,
                    navigation_error: Some(err.to_string()),
                },
            }
        })
        .collect()
}

fn fetch_result(cli: &Cli, prepared: PreparedResult, max_chars: usize) -> FullResult {
    let PreparedResult {
        item,
        page,
        goto_seconds,
        navigation_error,
    } = prepared;

    if let Some(error) = navigation_error {
        return FullResult {
            title: item.title,
            url: item.url,
            snippet: item.snippet,
            sources: item.sources,
            fetch_status: "error".to_string(),
            fetch_method: None,
            content: None,
            error: Some(format!("Navigation failed: {error}")),
        };
    }

    let readability = read_page(cli, &item.url, &page, max_chars, goto_seconds)
        .map(|read| read.text)
        .map_err(|err| err.to_string());
    let fallback_url = item.url.clone();
    resolve_fetch(item, readability, || {
        text_page(cli, &fallback_url, &page, "main", max_chars, goto_seconds)
            .map(|text| text.text)
            .map_err(|err| err.to_string())
    })
}

fn resolve_fetch<F>(
    item: SearchResult,
    readability: Result<String, String>,
    fallback: F,
) -> FullResult
where
    F: FnOnce() -> Result<String, String>,
{
    match readability {
        Ok(content) => FullResult {
            title: item.title,
            url: item.url,
            snippet: item.snippet,
            sources: item.sources,
            fetch_status: "success".to_string(),
            fetch_method: Some("readability".to_string()),
            content: Some(content),
            error: None,
        },
        Err(read_error) => match fallback() {
            Ok(content) => FullResult {
                title: item.title,
                url: item.url,
                snippet: item.snippet,
                sources: item.sources,
                fetch_status: "success".to_string(),
                fetch_method: Some("text_main_fallback".to_string()),
                content: Some(content),
                error: None,
            },
            Err(text_error) => FullResult {
                title: item.title,
                url: item.url,
                snippet: item.snippet,
                sources: item.sources,
                fetch_status: "error".to_string(),
                fetch_method: None,
                content: None,
                error: Some(format!(
                    "Readability failed: {read_error}; main-text fallback failed: {text_error}"
                )),
            },
        },
    }
}

fn read_results_concurrently(
    cli: &Cli,
    items: Vec<SearchResult>,
    max_chars: usize,
    concurrency: usize,
    read_timeout: Option<u64>,
) -> Vec<FullResult> {
    let mut cli = cli.clone();
    if let Some(read_timeout) = read_timeout {
        cli.command_timeout = read_timeout;
        cli.chrome_timeout = read_timeout;
    }

    // Chrome profiles share a page registry. Create pages sequentially so parallel
    // navigations cannot overwrite each other's page registrations.
    let prepared = prepare_results(&cli, items);
    let mut out = Vec::with_capacity(prepared.len());
    for chunk in prepared.chunks(concurrency) {
        let mut handles = Vec::with_capacity(chunk.len());
        for prepared in chunk.iter().cloned() {
            let cli = cli.clone();
            handles.push(thread::spawn(move || {
                fetch_result(&cli, prepared, max_chars)
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
                    fetch_method: None,
                    content: None,
                    error: Some("reader thread panicked".to_string()),
                }),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_result() -> SearchResult {
        SearchResult::new(
            "Example".to_string(),
            "https://example.com".to_string(),
            "Description".to_string(),
        )
    }

    #[test]
    fn readability_success_does_not_run_fallback() {
        let result = resolve_fetch(search_result(), Ok("article text".to_string()), || {
            panic!("fallback must not run after successful Readability extraction")
        });

        assert_eq!(result.fetch_status, "success");
        assert_eq!(result.fetch_method.as_deref(), Some("readability"));
        assert_eq!(result.content.as_deref(), Some("article text"));
        assert!(result.error.is_none());
    }

    #[test]
    fn readability_failure_uses_main_text_fallback() {
        let result = resolve_fetch(search_result(), Err("read failed".to_string()), || {
            Ok("main text".to_string())
        });

        assert_eq!(result.fetch_status, "success");
        assert_eq!(result.fetch_method.as_deref(), Some("text_main_fallback"));
        assert_eq!(result.content.as_deref(), Some("main text"));
        assert!(result.error.is_none());
    }

    #[test]
    fn both_extraction_failures_are_reported() {
        let result = resolve_fetch(search_result(), Err("read failed".to_string()), || {
            Err("main failed".to_string())
        });

        assert_eq!(result.fetch_status, "error");
        assert!(result.fetch_method.is_none());
        assert!(result.content.is_none());
        let error = result.error.expect("combined error");
        assert!(error.contains("Readability failed: read failed"));
        assert!(error.contains("main-text fallback failed: main failed"));
    }
}
