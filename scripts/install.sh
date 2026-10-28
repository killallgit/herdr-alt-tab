#!/bin/sh
# Puts the herdr-alt-tab binary at target/release/herdr-alt-tab, where
# herdr-plugin.toml expects it. Downloads the prebuilt binary published for this
# version, and compiles from source only when there isn't one for this machine.
set -eu

cd "$(dirname "$0")/.."

REPO="killallgit/herdr-alt-tab"
BIN="herdr-alt-tab"
DEST="target/release/$BIN"

compile() {
    echo "$1; compiling from source" >&2
    command -v cargo >/dev/null 2>&1 || {
        echo "cargo not found: install Rust from https://rustup.rs and try again" >&2
        exit 1
    }
    exec cargo build --release --locked --target-dir target
}

target_triple() {
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) echo aarch64-apple-darwin ;;
        Darwin-x86_64) echo x86_64-apple-darwin ;;
        Linux-x86_64) echo x86_64-unknown-linux-gnu ;;
        Linux-aarch64 | Linux-arm64) echo aarch64-unknown-linux-gnu ;;
        *) return 1 ;;
    esac
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' herdr-plugin.toml | head -n 1)
[ -n "$version" ] || compile "Could not read the version from herdr-plugin.toml"
triple=$(target_triple) || compile "No prebuilt binary for $(uname -s) $(uname -m)"
command -v curl >/dev/null 2>&1 || compile "curl not found"

asset="$BIN-$triple.tar.gz"
url="https://github.com/$REPO/releases/download/v$version/$asset"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

curl -fsSL -o "$scratch/$asset" "$url" ||
    compile "No release binary at $url"
curl -fsSL -o "$scratch/$asset.sha256" "$url.sha256" ||
    compile "No checksum at $url.sha256"

expected=$(cut -d ' ' -f 1 "$scratch/$asset.sha256")
actual=$(sha256_of "$scratch/$asset")
[ "$expected" = "$actual" ] || {
    echo "Checksum mismatch for $asset: expected $expected, got $actual" >&2
    exit 1
}

tar -xzf "$scratch/$asset" -C "$scratch" "$BIN"
mkdir -p "$(dirname "$DEST")"
mv "$scratch/$BIN" "$DEST"
chmod +x "$DEST"
echo "Installed $BIN v$version ($triple)"
