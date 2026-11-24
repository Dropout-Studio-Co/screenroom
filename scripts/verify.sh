#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'USAGE'
usage: scripts/verify.sh [--fast | --full]

Run Screenroom's local verification.

  --fast   comments, formatting, clippy, and workspace tests (default)
  --full   everything --fast runs, then the macOS hidden-window routing
           checks, cargo audit and cargo deny check when those tools exist
           and deny.toml is present
  -h, --help
           print this help and exit

The repository root is resolved from this script's own location, and the
script runs from that root. SCREENROOM_ALLOW_FOREGROUND is exported as 0 so
no child check can escalate to foreground UI automation.
USAGE
}

mode=""
for arg in "$@"; do
	case "$arg" in
		-h | --help)
			usage
			exit 0
			;;
		--fast | --full)
			if [ -n "$mode" ]; then
				echo "verify: choose a single mode flag; got '--$mode' and '$arg'." >&2
				usage >&2
				exit 2
			fi
			mode="${arg#--}"
			;;
		*)
			echo "verify: unknown argument '$arg'." >&2
			usage >&2
			exit 2
			;;
	esac
done
if [ -z "$mode" ]; then
	mode="fast"
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"

export SCREENROOM_ALLOW_FOREGROUND=0

skipped_steps=()

skip_step() {
	echo "SKIP: $1"
	skipped_steps+=("$1")
}

run_step() {
	local label="$1"
	shift
	local started=$SECONDS
	local status=0
	echo "==> $label"
	"$@" || status=$?
	if [ "$status" -ne 0 ]; then
		echo "FAIL: $label (exit $status)" >&2
		exit "$status"
	fi
	echo "PASS: $label ($((SECONDS - started))s)"
}

if ! command -v python3 >/dev/null 2>&1; then
	echo "verify: python3 is required to run scripts/no_comments.py check." >&2
	exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
	if [ -n "${HOME:-}" ] && [ -x "$HOME/.cargo/bin/cargo" ]; then
		PATH="$HOME/.cargo/bin:$PATH"
		export PATH
	fi
fi
if ! command -v cargo >/dev/null 2>&1; then
	echo "verify: cargo is required; install the Rust toolchain or add cargo to PATH." >&2
	exit 1
fi
if ! cargo fmt --version >/dev/null 2>&1; then
	echo "verify: rustfmt is required by 'cargo fmt --all --check'; install it with rustup component add rustfmt." >&2
	exit 1
fi
if ! cargo clippy --version >/dev/null 2>&1; then
	echo "verify: clippy is required by 'cargo clippy --workspace --all-targets'; install it with rustup component add clippy." >&2
	exit 1
fi

run_step "python3 scripts/no_comments.py check" python3 scripts/no_comments.py check
run_step "cargo fmt --all --check" cargo fmt --all --check
run_step "cargo clippy --workspace --all-targets -- -D warnings" cargo clippy --workspace --all-targets -- -D warnings
if command -v cargo-nextest >/dev/null 2>&1; then
	run_step "cargo nextest run --workspace" cargo nextest run --workspace
	run_step "cargo test --workspace --doc" cargo test --workspace --doc
else
	run_step "cargo test --workspace" cargo test --workspace
fi

if [ "$mode" = "full" ]; then
	if [ "$(uname -s)" = "Darwin" ]; then
		if [ -n "${CARGO_TARGET_DIR:-}" ] && [ "${CARGO_TARGET_DIR%/}" != "$root/target" ]; then
			echo "verify: warning: CARGO_TARGET_DIR is $CARGO_TARGET_DIR, but the routing checks run target/debug/screenroom-desktop." >&2
		fi
		run_step "scripts/check-quit-routing.sh" scripts/check-quit-routing.sh
		run_step "scripts/check-terminate-routing.sh" scripts/check-terminate-routing.sh
	else
		skip_step "routing checks (macOS only; this host is $(uname -s))"
	fi
	if command -v cargo-audit >/dev/null 2>&1; then
		run_step "cargo audit" cargo audit
	else
		skip_step "cargo audit (cargo-audit not installed)"
	fi
	if command -v cargo-deny >/dev/null 2>&1; then
		if [ -f "$root/deny.toml" ]; then
			run_step "cargo deny check" cargo deny check
		else
			skip_step "cargo deny check (deny.toml not found)"
		fi
	else
		skip_step "cargo deny check (cargo-deny not installed)"
	fi
fi

if [ "${#skipped_steps[@]}" -eq 0 ]; then
	echo "verify ($mode): PASS"
else
	summary=""
	for step in "${skipped_steps[@]}"; do
		if [ -z "$summary" ]; then
			summary="$step"
		else
			summary="$summary; $step"
		fi
	done
	echo "verify ($mode): PASS (skipped: $summary)"
fi
