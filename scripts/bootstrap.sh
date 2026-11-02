#!/usr/bin/env bash
# just bootstrap — fetch the pinned toolchains that are not managed by rustup/pnpm.
#
# PROJECT_CHARTER.md §3.7: "Before executing remote install scripts, download them
# from the official source, inspect them, and record the installed version."
# This script therefore downloads *release tarballs* and unpacks them into a
# repo-local prefix. It never pipes a remote script into a shell, never uses
# sudo, and never writes outside $REPO_ROOT/.toolchain.
#
# Everything it installs lands in .toolchain/ (gitignored). Removing that
# directory fully reverses this script.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TC="$REPO_ROOT/.toolchain"
DIST="$TC/dist"
PREFIX="$TC/prefix"

# Pinned in tools/versions.lock. Koka was verified 2026-08-05; Wasmtime was
# advanced on 2026-10-01 by ADR-0116 after two RustSec advisories, on
# 2026-10-07 by ADR-0244 after three more, on 2026-10-14 by ADR-0335 after
# RUSTSEC-2026-0353 to 0364, on 2026-10-20 by ADR-0363 after
# RUSTSEC-2026-0378 and 0379, and on 2026-11-02 by ADR-0363's amendment after
# RUSTSEC-2026-0422 to 0424.
KOKA_VERSION="3.2.3"
WASMTIME_VERSION="48.0.8"

# SHA-256 of the exact artifacts accepted by this repository. Koka's values were
# computed from downloaded artifacts on 2026-08-06. Wasmtime 48.0.8's values
# were pinned from immutable GitHub release-asset digests on 2026-11-02;
# bootstrap still verifies every downloaded byte before installing it.
arch="$(uname -m)"
os="$(uname -s)"

case "$os/$arch" in
    Darwin/arm64)
        KOKA_ASSET="koka-v${KOKA_VERSION}-macos-arm64.tar.gz"
        KOKA_SHA256="ffe84e8c679876894ac67da23066ceae1fd433e244038a7c884a4ca8b4698eb8"
        WASMTIME_DIR="wasmtime-v${WASMTIME_VERSION}-aarch64-macos"
        WASMTIME_SHA256="17a85a062b06de38c479923ee6fca2182c998484ce71bf49a572d42926c4f1db"
        ;;
    Linux/x86_64)
        KOKA_ASSET="koka-v${KOKA_VERSION}-linux-x64.tar.gz"
        KOKA_SHA256="e82a4b497f1f8791ee171d06c45293ba16432e485d645ddd9688bafa6ccde5a5"
        WASMTIME_DIR="wasmtime-v${WASMTIME_VERSION}-x86_64-linux"
        WASMTIME_SHA256="8716ce7bffa19d893b5b6dc01d8e298b1a81ab75a0455b5a2933156ea34a456e"
        ;;
    Linux/aarch64 | Linux/arm64)
        KOKA_ASSET="koka-v${KOKA_VERSION}-linux-arm64.tar.gz"
        KOKA_SHA256="ce0cf566ce2bd1dd3b4fbcc1d07f92d54ff57fb3cc6d6fa0728d4a72df464c02"
        WASMTIME_DIR="wasmtime-v${WASMTIME_VERSION}-aarch64-linux"
        WASMTIME_SHA256="8d82da4d2ac187ed8200689434ef573fc15b513d7841aea8d1f7ce4bb7c295d0"
        ;;
    *)
        echo "bootstrap: no pinned artifacts for $os/$arch." >&2
        echo "  supported: Darwin/arm64, Linux/x86_64, Linux/aarch64" >&2
        echo "  Add the platform with a checksum computed from the downloaded file." >&2
        exit 1
        ;;
esac
WASMTIME_ASSET="${WASMTIME_DIR}.tar.xz"

mkdir -p "$DIST" "$PREFIX/bin"

# `shasum` on macOS, `sha256sum` on most Linux images. Both print the digest
# first, so the parse is the same.
sha256_of() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        sha256sum "$1" | cut -d' ' -f1
    fi
}

verify() {  # verify <file> <expected-sha256>
    local actual
    actual="$(sha256_of "$1")"
    if [ "$actual" != "$2" ]; then
        echo "bootstrap: CHECKSUM MISMATCH for $1" >&2
        echo "  expected $2" >&2
        echo "  actual   $actual" >&2
        echo "  Refusing to unpack. Upstream may have re-published, or the download is corrupt." >&2
        echo "  Investigate before changing the pin in this script." >&2
        exit 1
    fi
    echo "  checksum ok: $(basename "$1")"
}

# A release's download is asked again on a transient failure (a timeout, or
# HTTP 408, 429, 500, 502, 503 or 504): one 500 from GitHub's releases failed
# a browser job of run 37934455275 before it ran a test.

# --- Koka ------------------------------------------------------------------
if [ -x "$PREFIX/bin/koka" ] && "$PREFIX/bin/koka" --version 2>/dev/null | grep -q "$KOKA_VERSION"; then
    echo "koka $KOKA_VERSION already present"
else
    tarball="$DIST/$KOKA_ASSET"
    url="https://github.com/koka-lang/koka/releases/download/v${KOKA_VERSION}/${KOKA_ASSET}"
    echo "fetching koka $KOKA_VERSION"
    [ -f "$tarball" ] || curl -sSL --fail --retry 4 --retry-delay 5 --max-time 300 -o "$tarball" "$url"
    verify "$tarball" "$KOKA_SHA256"
    tar xzf "$tarball" -C "$PREFIX"
    echo "  installed: $("$PREFIX/bin/koka" --version | head -1)"
fi

# --- Wasmtime --------------------------------------------------------------
if [ -x "$PREFIX/bin/wasmtime" ] && "$PREFIX/bin/wasmtime" --version 2>/dev/null | grep -q "$WASMTIME_VERSION"; then
    echo "wasmtime $WASMTIME_VERSION already present"
else
    tarball="$DIST/$WASMTIME_ASSET"
    url="https://github.com/bytecodealliance/wasmtime/releases/download/v${WASMTIME_VERSION}/${WASMTIME_ASSET}"
    echo "fetching wasmtime $WASMTIME_VERSION"
    [ -f "$tarball" ] || curl -sSL --fail --retry 4 --retry-delay 5 --max-time 300 -o "$tarball" "$url"
    verify "$tarball" "$WASMTIME_SHA256"
    tar xJf "$tarball" -C "$DIST"
    cp "$DIST/$WASMTIME_DIR/wasmtime" "$PREFIX/bin/wasmtime"
    cp "$DIST/$WASMTIME_DIR/LICENSE" "$PREFIX/WASMTIME-LICENSE"
    echo "  installed: $("$PREFIX/bin/wasmtime" --version)"
fi

# --- Rust target -----------------------------------------------------------
if command -v rustup >/dev/null 2>&1; then
    if ! rustup target list --installed 2>/dev/null | grep -qx 'wasm32-wasip2'; then
        echo "adding rust target wasm32-wasip2"
        rustup target add wasm32-wasip2
    else
        echo "rust target wasm32-wasip2 already present"
    fi
fi

# --- Node workspace --------------------------------------------------------
if command -v pnpm >/dev/null 2>&1; then
    echo "installing node workspace dependencies (pnpm, frozen lockfile if present)"
    if [ -f "$REPO_ROOT/pnpm-lock.yaml" ]; then
        (cd "$REPO_ROOT" && pnpm install --frozen-lockfile)
    else
        (cd "$REPO_ROOT" && pnpm install)
    fi
else
    echo "pnpm not found — enable it with: corepack enable pnpm" >&2
fi

echo
echo "bootstrap: done. Everything above lives in .toolchain/ and node_modules/."
echo "Add to PATH for direct use:  export PATH=\"$PREFIX/bin:\$PATH\""
echo "Run 'just doctor' to verify."
