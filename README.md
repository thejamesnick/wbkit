# WBKit

Package web games and apps for [WebBox](https://webbox.fun), the console that runs in your browser.

WBKit turns a folder of web files (HTML, JavaScript, assets) and a `manifest.json` into a single `.wbx` package you upload at [publish.webbox.fun](https://publish.webbox.fun). It's one small program with no dependencies: you don't need Node, Rust or anything else installed.

## Install

**macOS and Linux**

```sh
curl -fsSL https://raw.githubusercontent.com/thejamesnick/wbkit/main/install.sh | sh
```

**Windows** (PowerShell)

```powershell
irm https://raw.githubusercontent.com/thejamesnick/wbkit/main/install.ps1 | iex
```

**npm**: coming soon as `@webboxfun/wbkit`.

Or download the file for your system from [Releases](https://github.com/thejamesnick/wbkit/releases/latest). Each release lists SHA-256 checksums in `SHA256SUMS`.

> The programs aren't code-signed yet. The install scripts avoid the warnings. If you download by hand: on macOS, run `xattr -d com.apple.quarantine wbkit-darwin-*`; on Windows, choose **More info → Run anyway**.

## Quick start

```sh
wbkit init ./my-game        # writes a starter manifest.json
# edit manifest.json: author, rating, category, permissions
wbkit bundle ./my-game      # writes my-game-1.0.0.wbx
wbkit validate my-game-1.0.0.wbx
```

Then upload the `.wbx` under **New release** at [publish.webbox.fun](https://publish.webbox.fun).

## Commands

| Command | What it does |
|---|---|
| `wbkit init [dir]` | Write a starter `manifest.json`, using your page title and description |
| `wbkit bundle <dir>` | Package the folder. `-o` output file, `-m` manifest path, `--reproducible` for byte-identical builds, `--compress 0-9`, `-v` to list files |
| `wbkit validate <file>` | Check every file, checksum and manifest rule, plus WebBox upload limits |
| `wbkit info <file>` | Show the listing details and files inside a package |

WBKit never packs `.DS_Store`, `._*` files, `Thumbs.db`, `.git`, `node_modules` or other `.wbx` files, and refuses native programs (`.exe`, `.dll`, `.sh` and similar). Point it at your build output, not your project root.

## Docs

- [Getting started](https://publish.webbox.fun/docs/getting-started)
- [Manifest reference](https://publish.webbox.fun/docs/manifest)
- [Engine guides](https://publish.webbox.fun/docs/engines): Unity, Godot, Phaser, plain HTML5
- [Publishing and updates](https://publish.webbox.fun/docs/publishing)

## Build from source

```sh
cargo build --release      # target/release/wbkit
cargo test
```

The `.wbx` format engine is the `wbkit_core` library in `src/wbx.rs`; the command line program is in `src/bin/wbkit`.

## License

MIT
