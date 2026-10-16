set shell := ["bash", "-euo", "pipefail", "-c"]

# rustup, bun, and the Linux release-binary installers drop tools here without touching shell profiles.
export PATH := env("HOME") + "/.cargo/bin:" + env("HOME") + "/.bun/bin:" + env("HOME") + "/.local/bin:" + env("PATH")

# just has no min-version setting; [parallel] needs 1.42.
_min_just := if semver_matches(`just --version | cut -d' ' -f2`, ">=1.42.0") == "true" { "" } else { error("just >= 1.42.0 is required") }

default:
    @just --list

# Install every dev tool. Safe to re-run.
setup:
    #!/usr/bin/env bash
    set -uo pipefail
    os="$(uname -s)"
    failures=0

    report() { printf '%-16s %s\n' "$1" "$2"; }
    has() { command -v "$1" >/dev/null 2>&1; }

    ensure() {
        if eval "$2" >/dev/null 2>&1; then
            report "$1" present
        elif eval "$3" >/dev/null 2>&1 && eval "$2" >/dev/null 2>&1; then
            report "$1" installed
        else
            report "$1" failed
            failures=$((failures + 1))
        fi
    }

    cargo_tool() {
        cargo binstall --no-confirm --locked "$1" || cargo install --locked "$1"
    }

    node24() {
        local bin
        for bin in "$(command -v node 2>/dev/null)" /opt/homebrew/opt/node@24/bin/node /usr/local/opt/node@24/bin/node "$HOME/.local/bin/node"; do
            [ -x "$bin" ] && [ "$("$bin" -p 'process.versions.node.split(".")[0]' 2>/dev/null)" = 24 ] && return 0
        done
        return 1
    }

    install_node24() {
        if [ "$os" = Darwin ]; then
            brew install node@24
        else
            local arch dist ver
            arch="$(uname -m | sed 's/x86_64/x64/;s/aarch64/arm64/')"
            dist="https://nodejs.org/dist/latest-v24.x"
            ver="$(curl -fsSL "$dist/SHASUMS256.txt" | grep -o "node-v24[^ ]*-linux-$arch.tar.xz" | head -n1)"
            mkdir -p "$HOME/.local/share/node24" "$HOME/.local/bin"
            curl -fsSL "$dist/$ver" | tar -xJ --strip-components=1 -C "$HOME/.local/share/node24"
            ln -sf "$HOME/.local/share/node24/bin/"{node,npm,npx} "$HOME/.local/bin/"
        fi
    }

    toolchain_present() {
        rustup toolchain list | grep -q "^$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
    }

    if [ "$os" = Darwin ]; then
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
    ensure cargo-deny "has cargo-deny" "cargo_tool cargo-deny"
    ensure cargo-machete "has cargo-machete" "cargo_tool cargo-machete"
    ensure sqlx-cli "has sqlx" "cargo_tool sqlx-cli"
    ensure bacon "has bacon" "cargo_tool bacon"
    ensure prek "has prek" \
        "$(brew_or prek 'curl --proto =https --tlsv1.2 -LsSf https://github.com/j178/prek/releases/latest/download/prek-installer.sh | sh')"
    ensure bun "has bun" "curl -fsSL https://bun.sh/install | bash"
    ensure node24 node24 install_node24
    ensure actionlint "has actionlint" \
        "$(brew_or actionlint 'mkdir -p ~/.local/bin && bash <(curl -sSfL https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash) latest ~/.local/bin')"

    if ! has claude; then
        echo "warning: claude is not on PATH; install Claude Code to run the agent"
    fi
    node_bin="$(command -v node 2>/dev/null || true)"
    if node24 && { [ -z "$node_bin" ] || [ "$("$node_bin" -p 'process.versions.node.split(".")[0]')" != 24 ]; }; then
        echo "warning: node 24 is installed but another node comes first on PATH"
    fi

    if [ "$failures" -gt 0 ]; then
        echo "$failures tool(s) failed to install"
        exit 1
    fi

# Run every quality gate.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo deny check
    cargo machete
    cargo nextest run --workspace --locked --no-tests=pass

# Apply automatic fixes.
fix:
    cargo fmt --all

# Run the server and restart it on change.
dev:
    bacon run-server
