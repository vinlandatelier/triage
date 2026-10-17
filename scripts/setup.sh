#!/usr/bin/env bash
# Install every dev tool. Safe to re-run. Called by `just setup`, which exports a PATH
# that already includes the installers' bin dirs and passes the caller's own PATH as
# TRIAGE_USER_PATH, so warnings can name tools a plain shell would not find.
# No -e: a failed install is counted and reported, and the rest still run.
set -uo pipefail
if [ -z "${TRIAGE_USER_PATH:-}" ]; then
    echo "run this through \`just setup\`" >&2
    exit 1
fi
cd "$(dirname "$0")/.." || exit 1
os="$(uname -s)"
failures=0
logs="$(mktemp -d)"
trap 'rmdir "$logs" 2>/dev/null || true' EXIT

report() { printf '%-16s %s\n' "$1" "$2"; }
has() { command -v "$1" >/dev/null 2>&1; }

ensure() {
    if eval "$2" >/dev/null 2>&1; then
        report "$1" present
    elif eval "$3" >"$logs/$1.log" 2>&1 && eval "$2" >/dev/null 2>&1; then
        report "$1" installed
    else
        report "$1" "failed (see $logs/$1.log)"
        failures=$((failures + 1))
    fi
}

# binstall's own compile fallback ignores our feature flags, so the source build stays ours.
cargo_tool() {
    local crate="$1"
    shift
    cargo binstall --no-confirm --locked --disable-strategies compile "$crate" ||
        cargo install --locked "$@" "$crate"
}

node_major() { "${1:-node}" -p 'process.versions.node.split(".")[0]' 2>/dev/null; }
node24() { [ "$(node_major)" = 24 ]; }

install_node24() {
    if [ "$os" = Darwin ]; then
        brew install node@24
    else
        local arch dist ver tmp
        arch="$(uname -m | sed 's/x86_64/x64/;s/aarch64/arm64/')"
        dist="https://nodejs.org/dist/latest-v24.x"
        tmp="$(mktemp -d)"
        curl -fsSL "$dist/SHASUMS256.txt" | grep -E " node-v24[^ ]*-linux-$arch\.tar\.xz$" | head -n1 >"$tmp/SHASUMS256.txt" &&
            ver="$(awk '{print $2}' "$tmp/SHASUMS256.txt")" &&
            [ -n "$ver" ] &&
            curl -fsSL -o "$tmp/$ver" "$dist/$ver" &&
            (cd "$tmp" && sha256sum -c SHASUMS256.txt) &&
            mkdir -p "$HOME/.local/share/node24" "$HOME/.local/bin" &&
            tar -xJf "$tmp/$ver" --strip-components=1 -C "$HOME/.local/share/node24" &&
            ln -sf "$HOME/.local/share/node24/bin/"{node,npm,npx} "$HOME/.local/bin/"
        local rc=$?
        rm -rf "$tmp"
        return "$rc"
    fi
}

# Without RUSTUP_AUTO_INSTALL=0, rustup installs the pinned toolchain while we only ask whether it exists.
toolchain_present() {
    local chan comp installed
    chan="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
    [ -n "$chan" ] || return 1
    RUSTUP_AUTO_INSTALL=0 rustup toolchain list | grep -q "^$chan-" || return 1
    installed="$(RUSTUP_AUTO_INSTALL=0 rustup component list --installed --toolchain "$chan")" || return 1
    for comp in $(sed -n 's/^components *= *\[\(.*\)\]/\1/p' rust-toolchain.toml | tr -d '" ' | tr , ' '); do
        grep -Eq "^$comp(-|$)" <<<"$installed" || return 1
    done
}

if [ "$os" = Darwin ]; then
    if ! has brew; then
        echo "error: Homebrew is required on macOS; install it from https://brew.sh"
        exit 1
    fi
    brew_or() { echo "brew install $1"; }
else
    brew_or() { echo "$2"; }
fi

ensure rustup "has rustup" \
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none"
# Installs the channel and components pinned in rust-toolchain.toml.
ensure toolchain toolchain_present "rustup toolchain install"
ensure cargo-binstall "has cargo-binstall" \
    "$(brew_or cargo-binstall 'curl -L --proto =https --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash')"
ensure cargo-nextest "has cargo-nextest" "cargo_tool cargo-nextest"
ensure sqlx-cli "has sqlx" "cargo_tool sqlx-cli --no-default-features --features sqlite,rustls"
ensure bacon "has bacon" "$(brew_or bacon 'cargo_tool bacon')"
ensure prek "has prek" \
    "$(brew_or prek 'curl --proto =https --tlsv1.2 -LsSf https://github.com/j178/prek/releases/latest/download/prek-installer.sh | sh -s -- --no-modify-path')"
# Not brew on macOS: brew cannot install the version pinned in .bun-version.
ensure bun "has bun" "curl -fsSL https://bun.sh/install | bash -s bun-v$(cat .bun-version)"
bun_ver="$(bun --version 2>/dev/null || true)"
if [ -n "$bun_ver" ] && [ "$bun_ver" != "$(cat .bun-version)" ]; then
    echo "warning: bun is $bun_ver but .bun-version pins $(cat .bun-version); CI uses the pin"
fi
ensure node24 node24 install_node24
ensure actionlint "has actionlint" \
    "$(brew_or actionlint 'mkdir -p ~/.local/bin && bash <(curl -sSfL https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash) latest ~/.local/bin')"

if ! has claude; then
    echo "warning: claude is not on PATH; install Claude Code to run the agent"
fi
# Git hooks and plain shells use the user's PATH, not the one exported above.
user_path="$TRIAGE_USER_PATH"
for tool in cargo just prek bun actionlint; do
    dir="$(dirname "$(command -v "$tool" 2>/dev/null || echo /)")"
    if has "$tool" && [[ ":$user_path:" != *":$dir:"* ]]; then
        echo "warning: $tool is in $dir, which is not on your shell PATH"
    fi
done
user_node="$(PATH="$user_path" command -v node 2>/dev/null || true)"
if node24 && { [ -z "$user_node" ] || [ "$(node_major "$user_node")" != 24 ]; }; then
    echo "warning: node 24 is installed but your shell PATH does not resolve node to it; add $(dirname "$(command -v node)") to PATH"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures tool(s) failed to install"
    exit 1
fi

bun install --frozen-lockfile
# The root has no node_modules/.bin under the isolated linker, so bunx would fetch the latest playwright.
(cd web && bunx playwright install chromium)
prek install --install-hooks
