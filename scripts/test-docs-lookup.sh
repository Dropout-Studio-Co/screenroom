#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cli="$repo_root/scripts/docs-lookup.sh"
work="$(mktemp -d "${TMPDIR:-/tmp}/docs-lookup-test.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

failures=0
passes=0

pass() { passes=$((passes + 1)); printf 'PASS %s\n' "$1"; }

fail_check() { failures=$((failures + 1)); printf 'FAIL %s: %s\n' "$1" "$2" >&2; }

run_status() {
  local name="$1" want="$2"
  shift 2
  set +e
  "$@" >"$work/stdout" 2>"$work/stderr"
  local got=$?
  set -e
  if [ "$got" -eq "$want" ]; then
    pass "$name (exit $got)"
  else
    fail_check "$name" "expected exit $want, got $got: $(head -n 3 "$work/stderr" | tr '\n' ' ')"
  fi
}

assert_contains() {
  local name="$1" needle="$2"
  local file="${3:-$work/stdout}"
  if grep -Fq -- "$needle" "$file"; then
    pass "$name"
  else
    fail_check "$name" "missing [$needle] in $(basename "$file")"
  fi
}

assert_absent() {
  local name="$1" needle="$2"
  local file="${3:-$work/stdout}"
  if grep -Fq -- "$needle" "$file"; then
    fail_check "$name" "unexpected [$needle] in $(basename "$file")"
  else
    pass "$name"
  fi
}

assert_line_count() {
  local name="$1" want="$2"
  local file="${3:-$work/stdout}"
  local got
  got="$(wc -l < "$file" | tr -d ' ')"
  if [ "$got" -eq "$want" ]; then
    pass "$name"
  else
    fail_check "$name" "expected $want lines, got $got"
  fi
}

assert_equal() {
  local name="$1" want="$2" got="$3"
  if [ "$got" = "$want" ]; then
    pass "$name"
  else
    fail_check "$name" "expected [$want], got [$got]"
  fi
}

printf '== CLI presence and help\n'
run_status "docs-lookup.sh is executable" 0 test -x "$cli"
run_status "help exits 0" 0 "$cli" --help
assert_contains "help lists version" "version"
assert_contains "help lists json" "json"
assert_contains "help lists header" "header"

printf '== Cargo.lock resolution\n'
run_status "iced resolves" 0 "$cli" version iced
assert_equal "iced locked version" "0.14.0" "$(cat "$work/stdout")"
run_status "wgpu resolves" 0 "$cli" version wgpu
assert_equal "wgpu locked version" "27.0.1" "$(cat "$work/stdout")"
run_status "rusqlite resolves" 0 "$cli" version rusqlite
assert_equal "rusqlite locked version" "0.41.1" "$(cat "$work/stdout")"
run_status "libloading lists both versions" 0 "$cli" version libloading
assert_equal "libloading versions" "0.8.9
0.9.0" "$(cat "$work/stdout")"
run_status "unknown crate exits 3" 3 "$cli" version not-a-real-crate-xyz
run_status "version absent from lockfile exits 3" 3 env SCREENROOM_DOCS_CACHE="$work/empty-cache" "$cli" json iced --version 9.9.9 --offline

printf '== Argument validation\n'
run_status "empty crate rejected" 2 "$cli" version ""
run_status "path traversal crate rejected" 2 "$cli" version ../etc/passwd
run_status "shell metachar crate rejected" 2 "$cli" version "iced;id"
run_status "command substitution crate rejected" 2 "$cli" version '$(whoami)'
run_status "unknown option rejected" 2 "$cli" version iced --bogus
run_status "missing subcommand rejected" 2 "$cli"

empty_cache="$work/empty-cache"
run_status "offline cache miss on multi-version crate exits 4 after workspace resolution" 4 env SCREENROOM_DOCS_CACHE="$empty_cache" "$cli" search libloading Library --offline
run_status "ambiguous multi-version crate without workspace reference exits 2" 2 env SCREENROOM_DOCS_CACHE="$empty_cache" "$cli" search syn helper --offline

printf '== Rustdoc JSON search and item lookup on a seeded cache\n'
cache="$work/cache"
mkdir -p "$cache/rusqlite/0.41.1"
python3 - "$cache/rusqlite/0.41.1/default.json" <<'PY'
import json
import sys

fixture = {
    "root": 0,
    "crate_version": "0.41.1",
    "format_version": 61,
    "target": {"triple": "aarch64-apple-darwin"},
    "index": {
        "0": {
            "id": 0,
            "name": "rusqlite",
            "docs": None,
            "visibility": "public",
            "inner": {"module": {"is_crate": True, "items": [1, 3], "is_stripped": False}},
            "span": None,
        },
        "1": {
            "id": 1,
            "name": "Connection",
            "docs": "A connection to a SQLite database.\n\nCreated by opening a file.",
            "visibility": "public",
            "inner": {
                "struct": {
                    "kind": {"plain": {"fields": [], "has_stripped_fields": False}},
                    "generics": {"params": [], "where_predicates": []},
                    "impls": [10],
                }
            },
            "span": None,
        },
        "2": {
            "id": 2,
            "name": "execute",
            "docs": "Executes a statement on the connection.",
            "visibility": "public",
            "inner": {
                "function": {
                    "sig": {"inputs": [], "output": None, "is_c_variadic": False},
                    "generics": {"params": [], "where_predicates": []},
                    "header": {"is_const": False, "is_unsafe": False, "is_async": False, "abi": "Rust"},
                    "has_body": True,
                }
            },
            "span": None,
        },
        "3": {
            "id": 3,
            "name": "Error",
            "docs": "A SQLite error.",
            "visibility": "public",
            "inner": {
                "enum": {
                    "generics": {"params": [], "where_predicates": []},
                    "has_stripped_variants": False,
                    "variants": [4],
                }
            },
            "span": None,
        },
        "4": {
            "id": 4,
            "name": "SqliteFailure",
            "docs": "An error reported by SQLite itself.",
            "visibility": "public",
            "inner": {"variant": {"kind": "tuple", "fields": []}},
            "span": None,
        },
        "10": {
            "id": 10,
            "name": None,
            "docs": None,
            "visibility": "default",
            "inner": {
                "impl": {
                    "is_unsafe": False,
                    "generics": {"params": [], "where_predicates": []},
                    "provided_trait_methods": [],
                    "trait": None,
                    "for": {"resolved_path": {"path": "Connection", "id": 1, "args": None}},
                    "items": [2],
                    "is_negative": False,
                    "is_synthetic": False,
                }
            },
            "span": None,
        },
    },
    "paths": {
        "0": {"crate_id": 0, "path": ["rusqlite"], "kind": "module"},
        "1": {"crate_id": 0, "path": ["rusqlite", "Connection"], "kind": "struct"},
        "2": {"crate_id": 0, "path": ["rusqlite", "Connection", "execute"], "kind": "function"},
        "3": {"crate_id": 0, "path": ["rusqlite", "Error"], "kind": "enum"},
        "4": {"crate_id": 0, "path": ["rusqlite", "Error", "SqliteFailure"], "kind": "variant"},
    },
}

with open(sys.argv[1], "w", encoding="utf-8") as handle:
    json.dump(fixture, handle)
PY

run_status "offline search reads seeded cache" 0 env SCREENROOM_DOCS_CACHE="$cache" "$cli" search rusqlite connection --offline
assert_contains "search finds the struct" "struct rusqlite::Connection"
assert_contains "search prints the first doc line" "A connection to a SQLite database."
assert_line_count "search default limit lists both matches" 2
run_status "search honours limit flag" 0 env SCREENROOM_DOCS_CACHE="$cache" "$cli" search rusqlite connection --offline --limit 1
assert_line_count "search limit caps output" 1
run_status "item lookup by name" 0 env SCREENROOM_DOCS_CACHE="$cache" "$cli" item rusqlite Connection --offline
assert_contains "item prints kind" "kind: struct"
assert_contains "item prints full path" "path: rusqlite::Connection"
assert_contains "item prints visibility" "visibility: public"
assert_contains "item prints docs" "A connection to a SQLite database."
assert_contains "item prints methods" "execute"
run_status "item lookup by exact path" 0 env SCREENROOM_DOCS_CACHE="$cache" "$cli" item rusqlite rusqlite::Error --offline
assert_contains "item prints variants" "SqliteFailure"
run_status "missing item exits 3" 3 env SCREENROOM_DOCS_CACHE="$cache" "$cli" item rusqlite NoSuchItem --offline
run_status "offline json cache miss exits 4" 4 env SCREENROOM_DOCS_CACHE="$work/never" "$cli" json rusqlite --offline
run_status "json writes --out" 0 env SCREENROOM_DOCS_CACHE="$cache" "$cli" json rusqlite --offline --out "$work/out.json"
run_status "json --out file is valid JSON" 0 python3 -c 'import json, sys; json.load(open(sys.argv[1]))' "$work/out.json"
run_status "json --out - streams JSON" 0 env SCREENROOM_DOCS_CACHE="$cache" "$cli" json rusqlite --offline --out -
cp "$work/stdout" "$work/stream.json"
run_status "json --out - payload is valid JSON" 0 python3 -c 'import json, sys; json.load(open(sys.argv[1]))' "$work/stream.json"

printf '== mpv headers\n'
run_status "mpv header listing" 0 "$cli" header mpv
assert_contains "listing shows client.h" "client.h"
assert_contains "listing shows render.h" "render.h"
run_status "mpv grep for an event macro" 0 "$cli" header mpv client.h --grep MPV_EVENT_PLAYBACK_RESTART
assert_contains "grep finds the macro" "MPV_EVENT_PLAYBACK_RESTART"
run_status "mpv missing header exits 3" 3 "$cli" header mpv no-such-header.h
run_status "mpv traversal filename rejected" 2 "$cli" header mpv ../client.h
run_status "mpv absolute filename rejected" 2 "$cli" header mpv /etc/passwd
mkdir -p "$work/mpv"
ln -s /etc/passwd "$work/mpv/evil.h"
run_status "mpv symlink escape rejected" 2 env SCREENROOM_MPV_INCLUDE_DIR="$work/mpv" "$cli" header mpv evil.h
run_status "unknown header namespace rejected" 2 "$cli" header nope

printf '== Sparkle headers\n'
mkdir -p "$work/sparkle/Sparkle.framework/Headers"
printf '%s\n' '@interface Sparkle : NSObject' 'SU_EXPORT extern NSString *const SUUpdaterKey;' '@end' >"$work/sparkle/Sparkle.framework/Headers/Sparkle.h"
run_status "sparkle header listing from override" 0 env SCREENROOM_SPARKLE_DIR="$work/sparkle" "$cli" header sparkle
assert_contains "sparkle listing shows Sparkle.h" "Sparkle.h"
run_status "sparkle grep for a symbol" 0 env SCREENROOM_SPARKLE_DIR="$work/sparkle" "$cli" header sparkle Sparkle.h --grep SUUpdaterKey
assert_contains "sparkle grep finds the symbol" "SUUpdaterKey"
run_status "missing sparkle directory exits 5" 5 env SCREENROOM_SPARKLE_DIR="$work/no-sparkle-here" "$cli" header sparkle
if [ -d "$repo_root/vendor/sparkle/Sparkle.framework/Headers" ]; then
  run_status "vendored sparkle headers listed" 0 "$cli" header sparkle
  assert_contains "vendored listing shows Sparkle.h" "Sparkle.h"
fi

printf '== MCP configuration\n'
run_status "mcp.json parses" 0 python3 -m json.tool .mcp.json
run_status "opencode.json parses" 0 python3 -m json.tool opencode.json
run_status "mcp.json servers are structurally correct" 0 python3 -c 'import json; d = json.load(open(".mcp.json")); assert d["mcpServers"]["trail"] == {"type": "stdio", "command": "trail", "args": ["mcp"]}; assert d["mcpServers"]["docsrs"] == {"type": "stdio", "command": "docsrs-mcp", "args": []}'
run_status "opencode.json servers are structurally correct" 0 python3 -c 'import json; d = json.load(open("opencode.json")); assert d["mcp"]["trail"] == {"type": "local", "command": ["trail", "mcp"], "enabled": True}; assert d["mcp"]["docsrs"] == {"type": "local", "command": ["docsrs-mcp"], "enabled": True}'
run_status "opencode.json stays valid JSON after update" 0 python3 -m json.tool opencode.json
if command -v docsrs-mcp >/dev/null 2>&1; then
  set +e
  printf '%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"1.0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' | docsrs-mcp >"$work/mcp.out" 2>"$work/mcp.err"
  set -e
  if grep -Fq -- '"serverInfo"' "$work/mcp.out" && grep -Fq -- '"tools"' "$work/mcp.out"; then
    pass "docsrs-mcp answers stdio initialize"
  else
    fail_check "docsrs-mcp answers stdio initialize" "no initialize response on stdout"
  fi
else
  fail_check "docsrs-mcp answers stdio initialize" "docsrs-mcp is not installed; run cargo install --locked docsrs-mcp"
fi

printf '== Dependency policy file\n'
run_status "deny.toml parses" 0 python3 -c 'import tomllib; tomllib.load(open("deny.toml", "rb"))'
run_status "deny.toml matches the approved policy" 0 python3 -c 'import tomllib; d = tomllib.load(open("deny.toml", "rb")); assert d["graph"]["targets"] == ["aarch64-apple-darwin"]; assert d["graph"]["all-features"] is False; assert d["graph"]["no-default-features"] is False; assert d["advisories"]["yanked"] == "warn"; assert d["advisories"]["unmaintained"] == "all"; assert d["advisories"]["unsound"] == "all"; ids = sorted(entry["id"] for entry in d["advisories"]["ignore"]); assert ids == ["RUSTSEC-2024-0436", "RUSTSEC-2026-0192", "RUSTSEC-2026-0253"], ids; assert all(entry.get("reason") for entry in d["advisories"]["ignore"]); assert d["licenses"]["confidence-threshold"] == 0.93; allow = d["licenses"]["allow"]; assert "GPL-2.0-only" not in allow and "LGPL-2.1-or-later" not in allow and "MPL-2.0" not in allow; assert d["licenses"]["exceptions"] == [{"crate": "webpki-roots", "allow": ["CDLA-Permissive-2.0"]}]; assert d["bans"]["multiple-versions"] == "warn"; assert d["bans"]["wildcards"] == "deny"; assert d["sources"]["unknown-registry"] == "deny"; assert d["sources"]["unknown-git"] == "deny"; assert d["sources"]["allow-registry"] == ["https://github.com/rust-lang/crates.io-index"]; assert d["sources"]["allow-git"] == []'
run_status "cargo deny check passes" 0 cargo deny check

printf '== Repo hygiene\n'
run_status "no_comments check" 0 python3 scripts/no_comments.py check
run_status "all five new surfaces are comment-free" 0 python3 -c 'import importlib.util; spec = importlib.util.spec_from_file_location("nc", "scripts/no_comments.py"); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); checks = [(".mcp.json", "jsonc"), ("opencode.json", "jsonc"), ("deny.toml", "hash"), ("scripts/docs-lookup.sh", "shell"), ("scripts/test-docs-lookup.sh", "shell")]; bad = [(p, len(m.LEXERS[l](open(p).read()))) for p, l in checks if m.LEXERS[l](open(p).read())]; assert not bad, bad'

printf '== Live crates.io and docs.rs lookups\n'
live_cache="$work/live-cache"
run_status "meta iced from crates.io" 0 env SCREENROOM_DOCS_CACHE="$live_cache" "$cli" meta iced
set +e
python3 - "$work/stdout" <<'PY'
import json
import sys

meta = json.load(open(sys.argv[1]))
assert meta["name"] == "iced", meta["name"]
assert meta["version"] == "0.14.0", meta["version"]
assert meta["yanked"] is False
assert meta["license"] == "MIT"
assert meta["repository"] == "https://github.com/iced-rs/iced"
assert meta["docsrs_json_url"] == "https://docs.rs/crate/iced/0.14.0/json"
assert isinstance(meta["features"], list) and "default" in meta["features"]
PY
meta_status=$?
set -e
if [ "$meta_status" -eq 0 ]; then pass "meta fields are the locked crates.io values"; else fail_check "meta fields are the locked crates.io values" "metadata assertions failed"; fi
run_status "json libloading downloads and caches" 0 env SCREENROOM_DOCS_CACHE="$live_cache" "$cli" json libloading
cached_json="$(cat "$work/stdout")"
assert_contains "json prints the cache path" "libloading/0.9.0/default.json" "$work/stdout"
if [ -s "$cached_json" ]; then pass "cached rustdoc JSON exists"; else fail_check "cached rustdoc JSON exists" "cache file is empty or missing"; fi
run_status "search libloading Library against live docs.rs" 0 env SCREENROOM_DOCS_CACHE="$live_cache" "$cli" search libloading Library --offline --limit 200
assert_contains "live search finds Library" "libloading::safe::Library"
run_status "item libloading Library against live docs.rs" 0 env SCREENROOM_DOCS_CACHE="$live_cache" "$cli" item libloading Library --offline
assert_contains "live item prints the struct path" "path: libloading::safe::Library"
assert_contains "live item prints documentation" "library"
run_status "json with a target downloads target-specific rustdoc JSON" 0 env SCREENROOM_DOCS_CACHE="$live_cache" "$cli" json metal --target aarch64-apple-darwin
assert_contains "target cache path" "metal/0.32.0/aarch64-apple-darwin.json"
run_status "search with --target reads the target cache" 0 env SCREENROOM_DOCS_CACHE="$live_cache" "$cli" search metal Device --target aarch64-apple-darwin --offline --limit 5
assert_contains "target search finds device functions" "metal::device"

printf '\n%d passed, %d failed\n' "$passes" "$failures"
if [ "$failures" -ne 0 ]; then
  exit 1
fi
