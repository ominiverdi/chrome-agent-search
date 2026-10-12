use std::process::Command;

use anyhow::Result;

use crate::Cli;
use crate::chrome::run_cmd;
use crate::types::{CleanupOutput, CleanupRecord};

pub(crate) fn cleanup(
    cli: &Cli,
    purge: bool,
    orphans: bool,
    purge_orphans: bool,
) -> Result<CleanupOutput> {
    let mut records = Vec::new();
    if orphans || purge_orphans {
        let mut cmd = chrome_close_base(cli, &cli.browser);
        if orphans {
            cmd.arg("--orphans");
        }
        if purge_orphans {
            cmd.arg("--purge-orphans");
        }
        records.push(run_cleanup_command(cli, "orphans".to_string(), cmd));
    }
    for browser in cleanup_browser_names(&cli.browser) {
        let mut cmd = chrome_close_base(cli, &browser);
        if purge {
            cmd.arg("--purge");
        }
        records.push(run_cleanup_command(cli, browser, cmd));
    }
    let ok = records.iter().all(|record| record.ok);
    Ok(CleanupOutput { ok, records })
}

fn cleanup_browser_names(base: &str) -> Vec<String> {
    vec![
        base.to_string(),
        format!("{base}-bing"),
        format!("{base}-brave"),
        format!("{base}-duckduckgo"),
    ]
}

fn chrome_close_base(cli: &Cli, browser: &str) -> Command {
    let mut cmd = Command::new(&cli.chrome_agent);
    cmd.arg("--browser").arg(browser);
    for arg in &cli.chrome_arg {
        cmd.arg("--chrome-arg").arg(arg);
    }
    if cli.stealth {
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
    cmd.arg("close");
    cmd
}

fn run_cleanup_command(cli: &Cli, browser: String, cmd: Command) -> CleanupRecord {
    match run_cmd(cmd, cli.command_timeout) {
        Ok(output) => CleanupRecord {
            browser,
            ok: true,
            output,
            error: None,
        },
        Err(err) => CleanupRecord {
            browser,
            ok: false,
            output: String::new(),
            error: Some(err.to_string()),
        },
    }
}
