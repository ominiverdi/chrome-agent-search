use anyhow::{Context, Result, anyhow};
use sha1::{Digest, Sha1};
use std::fs;
use std::fs::File;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wait_timeout::ChildExt;

use crate::Cli;
use crate::types::{DiagnoseOutput, TimedText};

pub(crate) fn chrome_base(cli: &Cli, page: &str) -> Command {
    chrome_base_inner(cli, page, cli.stealth)
}

pub(crate) fn chrome_base_stealth(cli: &Cli, page: &str) -> Command {
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
    if let Some(proxy_server) = &cli.proxy_server {
        cmd.arg("--proxy-server").arg(proxy_server);
    }
    if let Some(connect) = &cli.connect {
        cmd.arg("--connect").arg(connect);
    }
    cmd
}

pub(crate) fn run_cmd(mut cmd: Command, timeout_secs: u64) -> Result<String> {
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

pub(crate) fn page_name(prefix: &str, value: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(value.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    format!("cas-{}-{}", prefix, &hash[..12])
}

pub(crate) fn goto(cli: &Cli, page: &str, url: &str) -> Result<(String, f64)> {
    let start = Instant::now();
    let mut cmd = chrome_base(cli, page);
    cmd.arg("goto").arg(url);
    let text = run_cmd(cmd, cli.command_timeout)?;
    Ok((text, start.elapsed().as_secs_f64()))
}

pub(crate) fn page_html(cli: &Cli, page: &str, stealth: bool) -> Result<String> {
    let mut cmd = if stealth {
        chrome_base_stealth(cli, page)
    } else {
        chrome_base(cli, page)
    };
    cmd.arg("eval").arg("document.documentElement.outerHTML");
    let raw = run_cmd(cmd, cli.command_timeout)?;
    Ok(serde_json::from_str::<String>(&raw).unwrap_or(raw))
}

pub(crate) fn diagnose_url(cli: &Cli, url: &str) -> Result<DiagnoseOutput> {
    let page = page_name("diagnose", url);
    let mut cmd = chrome_base(cli, &page);
    cmd.arg("--json").arg("goto").arg(url).arg("--inspect");
    let raw = run_cmd(cmd, cli.command_timeout)?;
    let json: serde_json::Value = serde_json::from_str(&raw).context("parse chrome-agent JSON")?;
    let landed = json.get("landed");
    let final_url = landed
        .and_then(|v| v.get("final"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let redirected = landed
        .and_then(|v| v.get("redirected"))
        .and_then(|v| v.as_bool());
    let http_status = landed
        .and_then(|v| v.get("http_status"))
        .and_then(|v| v.as_u64())
        .and_then(|v| u16::try_from(v).ok());
    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let snapshot_excerpt = json
        .get("snapshot")
        .and_then(|v| v.as_str())
        .map(|snapshot| snapshot.chars().take(500).collect::<String>());
    let hint = diagnose_hint(
        http_status,
        redirected,
        title.as_deref(),
        snapshot_excerpt.as_deref(),
    );
    Ok(DiagnoseOutput {
        ok: true,
        requested_url: url.to_string(),
        final_url,
        redirected,
        http_status,
        title,
        snapshot_excerpt,
        hint,
    })
}

fn diagnose_hint(
    http_status: Option<u16>,
    redirected: Option<bool>,
    title: Option<&str>,
    snapshot: Option<&str>,
) -> String {
    let title = title.unwrap_or_default().to_lowercase();
    let snapshot = snapshot.unwrap_or_default().to_lowercase();
    if http_status.map(|status| status >= 400).unwrap_or(false)
        || title.contains("human")
        || snapshot.contains("blocked")
        || snapshot.contains("captcha")
    {
        "The page may be blocked, protected, or requiring human verification. Try --headed or --connect with a real Chrome, complete any visible step manually, keep that browser open, then retry with the same --browser profile.".to_string()
    } else if redirected.unwrap_or(false) {
        "The page redirected before content was read. If it redirected to login or consent, open with --headed, complete the step manually, keep the browser open, then retry with the same --browser profile.".to_string()
    } else {
        "The page loaded. If read still fails, it may not be article-like; try text --selector body or inspect the page manually.".to_string()
    }
}

pub(crate) fn read_url(cli: &Cli, url: &str, max_chars: usize) -> Result<TimedText> {
    let page = page_name("read", url);
    let (_goto_text, goto_seconds) = goto(cli, &page, url)?;
    let start = Instant::now();
    let mut cmd = chrome_base(cli, &page);
    cmd.arg("read").arg("--truncate").arg(max_chars.to_string());
    let text = run_cmd(cmd, cli.command_timeout).map_err(improve_read_error)?;
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

fn improve_read_error(err: anyhow::Error) -> anyhow::Error {
    let message = err.to_string();
    if message.contains("Page has minimal readable content") {
        anyhow!(
            "readability_minimal_content: Chrome Readability could not extract main content. The page may be non-article content, a login/consent page, or a human-verification/block page. Try `text --selector body` or retry with `--headed` using the same `--browser` profile."
        )
    } else {
        err
    }
}

pub(crate) fn text_url(
    cli: &Cli,
    url: &str,
    selector: &str,
    max_chars: usize,
) -> Result<TimedText> {
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
