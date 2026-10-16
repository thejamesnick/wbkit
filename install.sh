#!/bin/sh
# WBKit installer for macOS and Linux: downloads the right program, checks its SHA-256, installs it.
# Options (environment): WBKIT_VERSION=v0.1.0 (default: latest), WBKIT_INSTALL_DIR (default: ~/.local/bin)
set -eu
REPO="thejamesnick/wbkit"
fail() { printf 'wbkit install: %s\n' "$1" >&2; exit 1; }

case "$(uname -s)" in
  Darwin) os=darwin ;;
  Linux) os=linux ;;
  *) fail "unsupported system $(uname -s). On Windows use install.ps1, or npm install -g @webboxfun/wbkit" ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x64 ;;
  arm64|aarch64) arch=arm64 ;;
  *) fail "unsupported processor $(uname -m)" ;;
esac
# A shell running under Rosetta reports x86_64 on Apple Silicon; prefer the native program.
if [ "$os" = darwin ] && [ "$arch" = x64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then arch=arm64; fi

asset="wbkit-$os-$arch"
version="${WBKIT_VERSION:-latest}"
if [ "$version" = latest ]; then base="https://github.com/$REPO/releases/latest/download"; else base="https://github.com/$REPO/releases/download/$version"; fi
dir="${WBKIT_INSTALL_DIR:-$HOME/.local/bin}"
command -v curl >/dev/null 2>&1 || fail "curl is required"

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
printf 'Downloading %s (%s)...\n' "$asset" "$version"
curl -fsSL "$base/$asset" -o "$tmp/$asset" || fail "download failed: $base/$asset"
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" || fail "checksum download failed"
expected="$(grep " $asset\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
[ -n "$expected" ] || fail "no checksum listed for $asset"
if command -v sha256sum >/dev/null 2>&1; then actual="$(sha256sum "$tmp/$asset" | cut -d' ' -f1)"; else actual="$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)"; fi
[ "$expected" = "$actual" ] || fail "checksum mismatch; the download may be damaged"

mkdir -p "$dir"
mv "$tmp/$asset" "$dir/wbkit"
chmod +x "$dir/wbkit"
printf 'Installed %s\n' "$("$dir/wbkit" --version)"
printf 'Location: %s/wbkit\n' "$dir"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) printf '\n%s is not on your PATH. Add this to your shell profile:\n  export PATH="%s:$PATH"\n' "$dir" "$dir" ;;
esac
