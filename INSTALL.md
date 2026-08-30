# Install

`chrome-agent-search` is a standalone Rust CLI that calls the `chrome-agent` executable.

## Requirements

- Chromium or Chrome installed.
- `chrome-agent` available in `PATH`.
- `chrome-agent-search` available in `PATH`.

## Install with Cargo

Install `chrome-agent`:

```sh
cargo install chrome-agent --locked
```

Install `chrome-agent-search`:

```sh
cargo install --git https://github.com/ominiverdi/chrome-agent-search --locked
```

Verify:

```sh
chrome-agent --version
chrome-agent-search --help
```

## Install from GitHub Releases

Download the archive for your platform from:

```text
https://github.com/ominiverdi/chrome-agent-search/releases
```

Release assets are named like:

```text
chrome-agent-search-linux-x86_64.tar.gz
chrome-agent-search-macos-arm64.tar.gz
chrome-agent-search-windows-x86_64.zip
SHA256SUMS
```

Unpack the archive and put the binary somewhere in `PATH`, for example:

```sh
tar -xzf chrome-agent-search-linux-x86_64.tar.gz
install -m 0755 chrome-agent-search ~/.local/bin/chrome-agent-search
```

You still need `chrome-agent` and Chromium/Chrome installed separately.

## Browser setup

By default, `chrome-agent-search` uses the default `chrome-agent` managed browser profile.

Useful options:

```sh
chrome-agent-search --browser research search 'query'
chrome-agent-search --stealth search 'query'
chrome-agent-search --copy-cookies read 'https://example.com/account'
chrome-agent-search --connect http://127.0.0.1:9222 read 'https://example.com'
```

## Cleanup

Close managed browser sessions:

```sh
chrome-agent-search cleanup
```

Delete managed browser profile data for the selected browser family:

```sh
chrome-agent-search cleanup --purge
```

Sweep old orphaned `chrome-agent` profiles:

```sh
chrome-agent-search cleanup --purge-orphans
```

`--purge` deletes cookies/cache/profile data for the selected browser names. Do not use it on a profile you want to keep.
