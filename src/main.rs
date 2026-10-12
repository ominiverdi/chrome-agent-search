use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

mod bench;
mod chrome;
mod cleanup;
mod full;
mod output;
mod parsers;
mod scoring;
mod search;
mod types;

use bench::run_bench;
use chrome::{diagnose_url, read_url, text_url};
use cleanup::cleanup;
use full::full_search;
use output::{print_cleanup, print_diagnose, print_full, print_read, print_search};
use search::search;

#[derive(Parser, Debug, Clone)]
#[command(name = "chrome-agent-search")]
#[command(about = "Search and read web pages through chrome-agent")]
#[command(version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, default_value = "chrome-agent", global = true)]
    pub(crate) chrome_agent: String,

    #[arg(long, default_value = "default", global = true)]
    pub(crate) browser: String,

    /// Repeatable Chrome launch option; requires chrome-agent >= 0.14.0.
    #[arg(long, global = true, allow_hyphen_values = true)]
    pub(crate) chrome_arg: Vec<String>,

    #[arg(long, global = true)]
    pub(crate) stealth: bool,

    #[arg(long, global = true)]
    pub(crate) copy_cookies: bool,

    #[arg(long, global = true)]
    pub(crate) headed: bool,

    #[arg(long, global = true)]
    pub(crate) connect: Option<String>,

    #[arg(long, global = true)]
    pub(crate) proxy_server: Option<String>,

    #[arg(
        long,
        global = true,
        help = "SearXNG-compatible base URL, e.g. http://search.home"
    )]
    pub(crate) searxng: Option<String>,

    #[arg(long, default_value_t = 30, global = true)]
    pub(crate) chrome_timeout: u64,

    #[arg(long, default_value_t = 90, global = true)]
    pub(crate) command_timeout: u64,

    #[arg(long, default_value_t = 5, global = true)]
    pub(crate) wait_timeout: u64,

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
    Diagnose {
        url: String,
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
    Cleanup {
        #[arg(long)]
        purge: bool,
        #[arg(long)]
        orphans: bool,
        #[arg(long)]
        purge_orphans: bool,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum Engine {
    Auto,
    All,
    Bing,
    Brave,
    Duckduckgo,
    Searxng,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.connect.is_some() && cli.proxy_server.is_some() {
        bail!(
            "--proxy-server cannot be used with --connect because proxy settings must be set when Chrome starts"
        );
    }
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
        Commands::Diagnose { url } => {
            let out = diagnose_url(&cli, url)?;
            print_diagnose(&out, cli.json)?;
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
        Commands::Cleanup {
            purge,
            orphans,
            purge_orphans,
        } => {
            let out = cleanup(&cli, *purge, *orphans, *purge_orphans)?;
            print_cleanup(&out, cli.json)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn chrome_args_are_repeatable_and_forwarded_without_splitting() {
        let cli = Cli::try_parse_from([
            "chrome-agent-search",
            "--chrome-arg",
            "--disable-dev-shm-usage",
            "--chrome-arg=--lang=en-US",
            "read",
            "https://example.com",
        ])
        .unwrap();
        assert_eq!(cli.chrome_arg, ["--disable-dev-shm-usage", "--lang=en-US"]);
        let cmd = chrome::chrome_base(&cli, "test");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|a| a == ["--chrome-arg", "--disable-dev-shm-usage"])
        );
        assert!(
            args.windows(2)
                .any(|a| a == ["--chrome-arg", "--lang=en-US"])
        );
    }

    #[test]
    fn default_does_not_add_chrome_launch_flags() {
        let cli =
            Cli::try_parse_from(["chrome-agent-search", "read", "https://example.com"]).unwrap();
        assert!(cli.chrome_arg.is_empty());
        assert!(
            !chrome::chrome_base(&cli, "test")
                .get_args()
                .any(|a| a == "--chrome-arg")
        );
    }
}
