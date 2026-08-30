use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::process::Command;
use std::time::Instant;

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
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Engine {
    Auto,
    Bing,
    Brave,
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
        Engine::Auto => {
            let bing = search_bing(cli, query, limit).ok();
            if let Some(ref out) = bing {
                if assess_quality(&out.results, query) >= 0.8 {
                    return Ok(out.clone());
                }
            }
            let brave = search_brave(cli, query, limit).ok();
            match (bing, brave) {
                (Some(b), Some(r)) => {
                    if assess_quality(&r.results, query) > assess_quality(&b.results, query) {
                        Ok(r)
                    } else {
                        Ok(b)
                    }
                }
                (Some(b), None) => Ok(b),
                (None, Some(r)) => Ok(r),
                (None, None) => Err(anyhow!("all search engines failed")),
            }
        }
    }
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
    let script = r#"
JSON.stringify([...document.querySelectorAll('[data-type=web]')].map(el => {
  const lines = (el.innerText || '').split('\n').map(s => s.trim()).filter(Boolean);
  const link = [...el.querySelectorAll('a[href]')].find(a => a.href.startsWith('http'));
  const url = link?.href || '';
  const title = lines.find((line, idx) => idx > 1 && line.length > 8 && !line.includes(' › ')) || (link?.textContent || '').trim();
  const snippet = lines.find((line, idx) => idx > 2 && line !== title && line.length > 40) || '';
  return { title, url, snippet };
}).filter(r => r.title && r.url && !r.url.includes('search.brave.com')).slice(0, 20))
"#;
    let mut cmd = chrome_base_stealth(cli, &page);
    cmd.arg("eval").arg(script);
    let raw = run(cmd)?;
    let decoded = serde_json::from_str::<String>(&raw).unwrap_or(raw);
    let mut results: Vec<SearchResult> = serde_json::from_str(&decoded).unwrap_or_default();
    results.truncate(limit);
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
    let script = r#"
JSON.stringify([...document.querySelectorAll('li.b_algo')].map(li => {
  function cleanUrl(href) {
    try {
      const u = new URL(href);
      if (u.hostname.includes('bing.com') && u.searchParams.get('u')) {
        let encoded = u.searchParams.get('u');
        if (encoded.startsWith('a1')) encoded = encoded.slice(2);
        encoded = encoded.replace(/-/g, '+').replace(/_/g, '/');
        try { return atob(encoded); } catch { return href; }
      }
    } catch {}
    return href;
  }
  const links = [...li.querySelectorAll('a[href]')].filter(a => a.textContent.trim().length > 0);
  const a = links.find(a => !a.textContent.includes('http')) || links[0];
  const title = (a?.textContent || '').trim();
  const url = cleanUrl(a?.href || '');
  const caption = li.querySelector('.b_caption p, p, .b_lineclamp2, .b_algoSlug')?.textContent || '';
  const snippet = caption.replace(/\s+/g, ' ').trim();
  return { title, url, snippet };
}).filter(r => r.title && r.url && !r.url.includes('bing.com')).slice(0, 20))
"#;
    let mut cmd = chrome_base(cli, &page);
    cmd.arg("eval").arg(script);
    let raw = run(cmd)?;
    let decoded = serde_json::from_str::<String>(&raw).unwrap_or(raw);
    let mut results: Vec<SearchResult> = serde_json::from_str(&decoded).unwrap_or_default();
    if results.is_empty() {
        results = parse_bing_text_fallback(cli, &page, limit)?;
    }
    results.truncate(limit);
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
    let words: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() > 2 && !common.contains(w))
        .map(ToString::to_string)
        .collect();
    if words.is_empty() {
        return 0.5;
    }
    let mut total = 0.0;
    for result in results {
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
        total += score.min(1.0);
    }
    total / results.len() as f64
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
