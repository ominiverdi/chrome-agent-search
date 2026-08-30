use std::thread;

use anyhow::Result;

use crate::chrome::read_url;
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
