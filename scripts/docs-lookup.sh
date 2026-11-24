#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

exec python3 - "$repo_root" "$@" <<'PY'
import gzip
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.error
import urllib.parse
import urllib.request

USAGE = """usage: docs-lookup.sh <command> [arguments]

Commands:
  version <crate>
  meta <crate> [--version <ver>] [--offline]
  json <crate> [--version <ver>] [--target <triple>] [--out <path>] [--no-cache] [--offline]
  search <crate> <query> [--version <ver>] [--target <triple>] [--limit <n>] [--no-cache] [--offline]
  item <crate> <path-or-name> [--version <ver>] [--target <triple>] [--no-cache] [--offline]
  header <mpv|sparkle> [filename] [--grep <pattern>] [--limit <n>]

Exit codes:
  0 success
  2 invalid arguments or ambiguous crate version
  3 crate, version, item or header not found
  4 network, HTTP, timeout or offline cache miss
  5 missing local dependency
"""

CRATE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$")
VERSION_RE = re.compile(r"^[0-9A-Za-z.+_-]{1,64}$")
TARGET_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
HEADER_RE = re.compile(r"^[A-Za-z0-9._-]+\.h$")

USER_AGENT = "screenroom-docs-lookup/1.0"
DOCS_ORIGIN = "https://docs.rs"
CRATES_ORIGIN = "https://crates.io"
ALLOWED_HOSTS = ("docs.rs", "crates.io")
MAX_COMPRESSED = 64 * 1024 * 1024
MAX_DECOMPRESSED = 128 * 1024 * 1024
DOC_FIRST_LINE = 160
ITEM_DOC_LINES = 40
ITEM_DOC_WIDTH = 400


class Failure(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


def parse_options(args, valued, flags):
    options = {}
    positional = []
    index = 0
    while index < len(args):
        argument = args[index]
        if argument in valued:
            if index + 1 >= len(args):
                raise Failure(2, f"{argument} requires a value")
            options[argument] = args[index + 1]
            index += 2
            continue
        if argument in flags:
            options[argument] = True
            index += 1
            continue
        if argument.startswith("--") and "=" in argument:
            name, value = argument.split("=", 1)
            if name in valued:
                options[name] = value
                index += 1
                continue
        if argument.startswith("-") and argument != "-":
            raise Failure(2, f"unknown option: {argument}")
        positional.append(argument)
        index += 1
    return options, positional


def require_crate(crate):
    if not CRATE_RE.match(crate):
        raise Failure(2, f"invalid crate name: {crate!r}")


def require_version(version):
    if not VERSION_RE.match(version):
        raise Failure(2, f"invalid version: {version!r}")


def require_target(target):
    if not TARGET_RE.match(target):
        raise Failure(2, f"invalid target triple: {target!r}")


def load_lock(repo):
    path = os.path.join(repo, "Cargo.lock")
    if not os.path.isfile(path):
        raise Failure(5, f"Cargo.lock not found at {path}")
    with open(path, "rb") as handle:
        return tomllib.load(handle)


def locked_versions(lock, crate):
    return sorted(
        package["version"] for package in lock.get("package", []) if package.get("name") == crate
    )


def workspace_versions(lock, crate, locked):
    versions = set()
    for package in lock.get("package", []):
        if "source" in package:
            continue
        for dependency in package.get("dependencies", []):
            parts = dependency.split()
            if parts and parts[0] == crate and len(parts) > 1 and parts[1] in locked:
                versions.add(parts[1])
    return versions


def resolve_version(lock, crate, explicit):
    locked = locked_versions(lock, crate)
    if not locked:
        raise Failure(3, f"{crate} is not in Cargo.lock")
    if explicit is not None:
        require_version(explicit)
        if explicit not in locked:
            raise Failure(3, f"{crate} {explicit} is not in Cargo.lock; locked: {', '.join(locked)}")
        return explicit
    if len(locked) == 1:
        return locked[0]
    direct = workspace_versions(lock, crate, set(locked))
    if len(direct) == 1:
        return next(iter(direct))
    raise Failure(2, f"ambiguous version for {crate}: {', '.join(locked)}; pass --version")


def request_timeout():
    raw = os.environ.get("SCREENROOM_DOCS_TIMEOUT", "15")
    try:
        value = float(raw)
    except ValueError:
        value = 15.0
    return min(120.0, max(1.0, value))


def https_get(url, limit):
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != "https" or parts.hostname not in ALLOWED_HOSTS:
        raise Failure(2, f"refusing URL outside the allowlisted HTTPS origins: {url}")
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=request_timeout()) as response:
            final = urllib.parse.urlsplit(response.geturl())
            if final.scheme != "https" or final.hostname not in ALLOWED_HOSTS:
                raise Failure(4, f"refusing redirect to {response.geturl()}")
            chunks = []
            total = 0
            while True:
                chunk = response.read(65536)
                if not chunk:
                    break
                total += len(chunk)
                if total > limit:
                    raise Failure(4, f"response exceeds {limit} bytes: {url}")
                chunks.append(chunk)
            return response.status, b"".join(chunks)
    except urllib.error.HTTPError as error:
        return error.code, b""
    except Failure:
        raise
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        raise Failure(4, f"request failed for {url}: {error}") from error


def gunzip(payload):
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(payload)) as stream:
            chunks = []
            total = 0
            while True:
                chunk = stream.read(65536)
                if not chunk:
                    break
                total += len(chunk)
                if total > MAX_DECOMPRESSED:
                    raise Failure(4, "decompressed rustdoc JSON exceeds 128 MiB")
                chunks.append(chunk)
            return b"".join(chunks)
    except Failure:
        raise
    except (OSError, EOFError) as error:
        raise Failure(4, f"could not decompress rustdoc JSON: {error}") from error


def unzstd(payload):
    binary = shutil.which("zstd")
    if not binary:
        raise Failure(4, "the .zst fallback needs zstd on PATH")
    result = subprocess.run([binary, "-dc"], input=payload, capture_output=True)
    if result.returncode != 0:
        raise Failure(4, "zstd could not decompress the rustdoc JSON payload")
    if len(result.stdout) > MAX_DECOMPRESSED:
        raise Failure(4, "decompressed rustdoc JSON exceeds 128 MiB")
    return result.stdout


def cache_root(repo):
    base = os.environ.get("SCREENROOM_DOCS_CACHE") or os.path.join(
        os.environ.get("TMPDIR") or "/tmp", "screenroom-docs-cache"
    )
    base = os.path.realpath(base)
    repository = os.path.realpath(repo)
    if base == repository or base.startswith(repository + os.sep):
        raise Failure(2, f"refusing a cache directory inside the repository: {base}")
    return base


def cache_path(repo, crate, version, target):
    return os.path.join(cache_root(repo), crate, version, (target or "default") + ".json")


def download_rustdoc(crate, version, target):
    base = f"{DOCS_ORIGIN}/crate/{crate}/{version}"
    if target:
        base = f"{base}/{target}"
    status, payload = https_get(base + "/json.gz", MAX_COMPRESSED)
    if status == 200:
        return gunzip(payload)
    if status in (404, 406) and shutil.which("zstd"):
        status, payload = https_get(base + "/json.zst", MAX_COMPRESSED)
        if status == 200:
            return unzstd(payload)
    label = f"{crate} {version}" + (f" {target}" if target else "")
    raise Failure(4, f"docs.rs returned HTTP {status} for {label}")


def ensure_rustdoc(repo, crate, version, target, no_cache, offline):
    path = cache_path(repo, crate, version, target)
    if not no_cache and os.path.isfile(path):
        return path
    if offline:
        raise Failure(4, f"offline and no cached rustdoc JSON at {path}")
    payload = download_rustdoc(crate, version, target)
    if no_cache:
        handle = tempfile.NamedTemporaryFile(
            prefix=f"{crate}-{version}-", suffix=".json", delete=False
        )
    else:
        directory = os.path.dirname(path)
        os.makedirs(directory, exist_ok=True)
        handle = tempfile.NamedTemporaryFile(
            prefix=".fetch-", suffix=".json", dir=directory, delete=False
        )
    try:
        handle.write(payload)
        handle.flush()
        os.fsync(handle.fileno())
        handle.close()
        if no_cache:
            return handle.name
        os.replace(handle.name, path)
    except BaseException:
        handle.close()
        if os.path.exists(handle.name):
            os.unlink(handle.name)
        raise
    return path


def load_document(path):
    try:
        with open(path, "rb") as handle:
            return json.load(handle)
    except (OSError, ValueError) as error:
        raise Failure(4, f"could not read rustdoc JSON at {path}: {error}") from error


def parse_limit(raw, default, cap):
    if raw is None:
        return default
    try:
        value = int(raw)
    except ValueError:
        raise Failure(2, f"invalid limit: {raw!r}") from None
    if value < 1:
        raise Failure(2, f"limit must be at least 1: {value}")
    return min(value, cap)


def item_kind(item):
    inner = item.get("inner") or {}
    for key in inner:
        return key
    return "item"


def first_doc_line(docs):
    if not docs:
        return ""
    for line in docs.splitlines():
        text = line.strip()
        if text:
            return text[:DOC_FIRST_LINE]
    return ""


def search_rows(document, crate):
    index = document.get("index") or {}
    paths = document.get("paths") or {}
    rows = []
    seen = set()
    for identifier, meta in paths.items():
        parts = [part for part in (meta.get("path") or []) if part]
        if not parts:
            continue
        item = index.get(identifier) or {}
        rows.append((meta.get("kind") or item_kind(item), "::".join(parts), item.get("docs") or ""))
        seen.add(identifier)
    for identifier, item in index.items():
        if identifier in seen:
            continue
        name = item.get("name")
        kind = item_kind(item)
        if not name or kind in ("impl", "module"):
            continue
        rows.append((kind, f"{crate}::{name}", item.get("docs") or ""))
    rows.sort(key=lambda row: (row[1], row[0]))
    return rows


def command_search(repo, args):
    options, positional = parse_options(
        args, {"--version", "--target", "--limit"}, {"--no-cache", "--offline"}
    )
    if len(positional) != 2:
        raise Failure(2, "search requires a crate and a query")
    crate, query = positional
    require_crate(crate)
    if not query.strip():
        raise Failure(2, "search query must not be empty")
    limit = parse_limit(options.get("--limit"), 25, 200)
    target = options.get("--target")
    if target is not None:
        require_target(target)
    lock = load_lock(repo)
    version = resolve_version(lock, crate, options.get("--version"))
    path = ensure_rustdoc(repo, crate, version, target, "--no-cache" in options, "--offline" in options)
    rows = search_rows(load_document(path), crate)
    needle = query.lower()
    matches = [row for row in rows if needle in row[1].lower() or needle in row[2].lower()]
    for kind, item_path, docs in matches[:limit]:
        line = first_doc_line(docs)
        if line:
            print(f"{kind} {item_path} \u2014 {line}")
        else:
            print(f"{kind} {item_path}")


def node_name(index, identifier):
    return (index.get(str(identifier)) or {}).get("name") or ""


def field_names(document, item):
    index = document.get("index") or {}
    inner = item.get("inner") or {}
    struct = inner.get("struct")
    if struct:
        kind = struct.get("kind")
        if isinstance(kind, dict):
            plain = kind.get("plain")
            if plain:
                return [node_name(index, field) for field in plain.get("fields") or []]
            if "tuple" in kind:
                return [node_name(index, field) for field in kind.get("tuple") or []]
        return []
    union = inner.get("union")
    if union:
        return [node_name(index, field) for field in union.get("fields") or []]
    return []


def variant_names(document, item):
    index = document.get("index") or {}
    enum = (item.get("inner") or {}).get("enum") or {}
    return [node_name(index, variant) for variant in enum.get("variants") or []]


def method_names(document, parts):
    paths = document.get("paths") or {}
    names = set()
    for meta in paths.values():
        path = [part for part in (meta.get("path") or []) if part]
        if len(path) == len(parts) + 1 and path[:-1] == parts:
            if meta.get("kind") in ("function", "method"):
                names.add(path[-1])
    return sorted(names)[:ITEM_DOC_LINES]


def find_item(document, query):
    index = document.get("index") or {}
    paths = document.get("paths") or {}
    entries = []
    seen = set()
    for identifier, meta in paths.items():
        parts = [part for part in (meta.get("path") or []) if part]
        if not parts:
            continue
        entries.append((identifier, parts, meta.get("kind") or "item"))
        seen.add(identifier)
    for identifier, item in index.items():
        if identifier in seen:
            continue
        name = item.get("name")
        if not name:
            continue
        entries.append((identifier, [name], item_kind(item)))
    for identifier, parts, kind in entries:
        if "::".join(parts) == query:
            return identifier, parts, kind
    for identifier, parts, kind in sorted(entries, key=lambda entry: (len(entry[1]), "::".join(entry[1]))):
        if "::".join(parts).endswith("::" + query) or parts[-1] == query:
            return identifier, parts, kind
    raise Failure(3, f"no item named {query} in the rustdoc JSON")


def command_item(repo, args):
    options, positional = parse_options(
        args, {"--version", "--target"}, {"--no-cache", "--offline"}
    )
    if len(positional) != 2:
        raise Failure(2, "item requires a crate and an item path or name")
    crate, query = positional
    require_crate(crate)
    if not query.strip():
        raise Failure(2, "item query must not be empty")
    target = options.get("--target")
    if target is not None:
        require_target(target)
    lock = load_lock(repo)
    version = resolve_version(lock, crate, options.get("--version"))
    path = ensure_rustdoc(repo, crate, version, target, "--no-cache" in options, "--offline" in options)
    document = load_document(path)
    identifier, parts, kind = find_item(document, query)
    item = (document.get("index") or {}).get(identifier) or {}
    kind = kind or item_kind(item)
    item_path = "::".join(parts)
    print(f"kind: {kind}")
    print(f"path: {item_path}")
    print(f"visibility: {item.get('visibility') or 'unknown'}")
    print("docs:")
    docs = (item.get("docs") or "").strip()
    lines = docs.splitlines() if docs else []
    for line in lines[:ITEM_DOC_LINES]:
        print(line[:ITEM_DOC_WIDTH])
    if len(lines) > ITEM_DOC_LINES:
        print("...")
    fields = field_names(document, item)
    variants = variant_names(document, item)
    methods = method_names(document, parts)
    print("fields: " + (", ".join(fields) if fields else "(none)"))
    print("variants: " + (", ".join(variants) if variants else "(none)"))
    print("methods: " + (", ".join(methods) if methods else "(none)"))


def write_out(source, destination):
    directory = os.path.dirname(os.path.abspath(destination))
    if not os.path.isdir(directory):
        raise Failure(2, f"output directory does not exist: {directory}")
    handle = tempfile.NamedTemporaryFile(prefix=".docs-out-", dir=directory, delete=False)
    try:
        with open(source, "rb") as reader:
            shutil.copyfileobj(reader, handle)
        handle.flush()
        os.fsync(handle.fileno())
        handle.close()
        os.replace(handle.name, destination)
    except BaseException:
        handle.close()
        if os.path.exists(handle.name):
            os.unlink(handle.name)
        raise


def command_json(repo, args):
    options, positional = parse_options(
        args, {"--version", "--target", "--out"}, {"--no-cache", "--offline"}
    )
    if len(positional) != 1:
        raise Failure(2, "json requires exactly one crate")
    crate = positional[0]
    require_crate(crate)
    target = options.get("--target")
    if target is not None:
        require_target(target)
    lock = load_lock(repo)
    version = resolve_version(lock, crate, options.get("--version"))
    path = ensure_rustdoc(repo, crate, version, target, "--no-cache" in options, "--offline" in options)
    destination = options.get("--out")
    if destination == "-":
        with open(path, "rb") as handle:
            sys.stdout.buffer.write(handle.read())
        sys.stdout.buffer.flush()
        return
    if destination:
        write_out(path, destination)
        print(os.path.abspath(destination))
        return
    print(path)


def command_version(repo, args):
    options, positional = parse_options(args, set(), set())
    if options:
        raise Failure(2, "version accepts a single crate name")
    if len(positional) != 1:
        raise Failure(2, "version requires exactly one crate")
    crate = positional[0]
    require_crate(crate)
    lock = load_lock(repo)
    locked = locked_versions(lock, crate)
    if not locked:
        raise Failure(3, f"{crate} is not in Cargo.lock")
    for version in locked:
        print(version)


def command_meta(repo, args):
    options, positional = parse_options(args, {"--version"}, {"--offline"})
    if len(positional) != 1:
        raise Failure(2, "meta requires exactly one crate")
    crate = positional[0]
    require_crate(crate)
    lock = load_lock(repo)
    version = resolve_version(lock, crate, options.get("--version"))
    if "--offline" in options:
        raise Failure(4, f"offline: crates.io metadata for {crate} {version} is not cached")
    url = f"{CRATES_ORIGIN}/api/v1/crates/{crate}/{version}"
    status, payload = https_get(url, MAX_COMPRESSED)
    if status != 200:
        raise Failure(4, f"crates.io returned HTTP {status} for {crate} {version}")
    try:
        envelope = json.loads(payload)
        declared = envelope["version"]
    except (ValueError, KeyError, TypeError) as error:
        raise Failure(4, f"could not parse crates.io metadata for {crate} {version}: {error}") from error
    metadata = {
        "name": declared.get("crate") or crate,
        "version": declared.get("num") or version,
        "yanked": bool(declared.get("yanked")),
        "license": declared.get("license"),
        "repository": declared.get("repository"),
        "documentation": declared.get("documentation"),
        "features": sorted(declared.get("features") or {}),
        "docsrs_json_url": f"{DOCS_ORIGIN}/crate/{crate}/{version}/json",
    }
    print(json.dumps(metadata, indent=2))


def header_root(repo, namespace):
    if namespace == "mpv":
        override = os.environ.get("SCREENROOM_MPV_INCLUDE_DIR")
        if override:
            if not os.path.isdir(override):
                raise Failure(5, f"mpv include directory does not exist: {override}")
            return os.path.realpath(override)
        candidates = [
            os.path.join(repo, "vendor", "lgpl", "prefix", "include", "mpv"),
            "/opt/homebrew/include/mpv",
            "/usr/local/include/mpv",
        ]
        for candidate in candidates:
            if os.path.isdir(candidate):
                return os.path.realpath(candidate)
        raise Failure(5, "mpv headers not found; set SCREENROOM_MPV_INCLUDE_DIR or build the media stack")
    if namespace == "sparkle":
        base = os.environ.get("SCREENROOM_SPARKLE_DIR") or os.path.join(repo, "vendor", "sparkle")
        root = os.path.join(base, "Sparkle.framework", "Headers")
        if not os.path.isdir(root):
            raise Failure(5, f"Sparkle headers not found at {root}; run scripts/fetch-sparkle.sh")
        return os.path.realpath(root)
    raise Failure(2, f"unknown header namespace: {namespace}")


def mpv_version(root):
    binary = shutil.which("pkg-config")
    if binary:
        result = subprocess.run([binary, "--modversion", "mpv"], capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip():
            return f"libmpv {result.stdout.strip()}"
    version_file = os.path.join(root, "VERSION")
    if os.path.isfile(version_file):
        with open(version_file, encoding="utf-8", errors="replace") as handle:
            text = handle.read().strip()
        if text:
            return f"libmpv {text}"
    return ""


def header_files(root):
    return sorted(
        name
        for name in os.listdir(root)
        if name.endswith(".h") and os.path.isfile(os.path.join(root, name))
    )


def header_path(root, name):
    if not HEADER_RE.match(name):
        raise Failure(2, f"invalid header filename: {name}")
    resolved = os.path.realpath(os.path.join(root, name))
    try:
        outside = os.path.commonpath([root, resolved]) != root
    except ValueError:
        outside = True
    if outside:
        raise Failure(2, f"header path escapes the header directory: {name}")
    if not os.path.isfile(resolved):
        raise Failure(3, f"header not found: {name}")
    return resolved


def grep_header(path, pattern, limit, label):
    try:
        compiled = re.compile(pattern)
    except re.error as error:
        raise Failure(2, f"invalid grep pattern: {error}") from error
    results = []
    with open(path, encoding="utf-8", errors="replace") as handle:
        for number, line in enumerate(handle, 1):
            if compiled.search(line):
                results.append(f"{label}:{number}: {line.rstrip()}")
                if len(results) >= limit:
                    break
    return results


def command_header(repo, args):
    options, positional = parse_options(args, {"--grep", "--limit"}, set())
    if not positional:
        raise Failure(2, "header requires the mpv or sparkle namespace")
    namespace = positional[0]
    if namespace not in ("mpv", "sparkle"):
        raise Failure(2, f"unknown header namespace: {namespace}")
    if len(positional) > 2:
        raise Failure(2, "header accepts at most one header filename")
    filename = positional[1] if len(positional) == 2 else None
    limit = parse_limit(options.get("--limit"), 100, 500)
    pattern = options.get("--grep")
    root = header_root(repo, namespace)
    if filename is None:
        if pattern is not None:
            results = []
            for name in header_files(root):
                results.extend(grep_header(header_path(root, name), pattern, limit - len(results), name))
                if len(results) >= limit:
                    break
            if not results:
                raise Failure(3, f"no header line matches {pattern!r}")
            for line in results:
                print(line)
            return
        for name in header_files(root):
            print(name)
        if namespace == "mpv":
            version = mpv_version(root)
            if version:
                print(version)
        return
    path = header_path(root, filename)
    if pattern is not None:
        results = grep_header(path, pattern, limit, filename)
        if not results:
            raise Failure(3, f"no line in {filename} matches {pattern!r}")
        for line in results:
            print(line)
        return
    with open(path, encoding="utf-8", errors="replace") as handle:
        sys.stdout.write(handle.read())


COMMANDS = {
    "version": command_version,
    "meta": command_meta,
    "json": command_json,
    "search": command_search,
    "item": command_item,
    "header": command_header,
}


def main(argv):
    if len(argv) < 1:
        print(USAGE, end="", file=sys.stderr)
        return 2
    repo = argv[0]
    if len(argv) < 2:
        print(USAGE, end="", file=sys.stderr)
        return 2
    command = argv[1]
    if command in ("-h", "--help", "help"):
        print(USAGE, end="")
        return 0
    handler = COMMANDS.get(command)
    if handler is None:
        print(f"unknown command: {command}", file=sys.stderr)
        print(USAGE, end="", file=sys.stderr)
        return 2
    try:
        handler(repo, argv[2:])
        return 0
    except Failure as error:
        print(error.message, file=sys.stderr)
        return error.code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
PY
