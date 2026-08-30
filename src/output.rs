use anyhow::Result;

use crate::types::{CleanupOutput, DiagnoseOutput, FullOutput, SearchOutput, TimedText};

pub(crate) fn print_read(out: &TimedText, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(out)?);
    } else {
        println!("{}", out.text);
    }
    Ok(())
}

pub(crate) fn print_cleanup(out: &CleanupOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(out)?);
    } else {
        for record in &out.records {
            if record.ok {
                println!("{}: ok", record.browser);
                if !record.output.is_empty() {
                    println!("{}", record.output);
                }
            } else {
                println!(
                    "{}: error: {}",
                    record.browser,
                    record.error.as_deref().unwrap_or("unknown error")
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn print_diagnose(out: &DiagnoseOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(out)?);
    } else {
        println!("URL: {}", out.requested_url);
        if let Some(final_url) = &out.final_url {
            println!("Final URL: {final_url}");
        }
        if let Some(status) = out.http_status {
            println!("HTTP status: {status}");
        }
        if let Some(redirected) = out.redirected {
            println!("Redirected: {redirected}");
        }
        if let Some(title) = &out.title {
            println!("Title: {title}");
        }
        if let Some(snapshot) = &out.snapshot_excerpt {
            println!("\nSnapshot excerpt:\n{snapshot}");
        }
        println!("\nHint: {}", out.hint);
    }
    Ok(())
}

pub(crate) fn print_full(out: &FullOutput, json: bool) -> Result<()> {
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

pub(crate) fn print_search(out: &SearchOutput, json: bool) -> Result<()> {
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
