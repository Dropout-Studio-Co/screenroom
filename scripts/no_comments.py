#!/usr/bin/env python3
USAGE = """usage: no_comments.py [check|strip]

check  exit 1 when any comment is found
strip  rewrite every file in place

Comments are lexed per language, not matched by pattern: a // inside a string, a
# inside ${var#pattern}, a /* inside a TypeScript regex literal and a Rust
lifetime that looks like an unterminated character literal are each told apart
from a real comment. Detection and rewriting share one lexer, so the check cannot
disagree with what the strip would leave behind.
"""

import os
import re
import subprocess
import sys

ROOTS = ("crates", "apps", "scripts")
ROOT_FILES = ("Cargo.toml",)

SKIP_PREFIXES = (
    "docs/",
    "reference/",
    "vendor/",
    ".trail/cache/",
    "target/",
    "node_modules/",
    "dist/",
    "build/",
    ".git/",
)

SKIP_SUFFIXES = (
    ".lock",
    ".png",
    ".ico",
    ".icns",
    ".webm",
    ".zip",
    ".jpg",
    ".jpeg",
    ".pdf",
)

SKIP_NAMES = ("LICENSE", "pnpm-lock.yaml")

LANGUAGES = {
    ".rs": "rust",
    ".ts": "js",
    ".js": "js",
    ".mjs": "js",
    ".cjs": "js",
    ".css": "c",
    ".swift": "swift",
    ".sh": "shell",
    ".html": "markup",
    ".xml": "markup",
    ".tproject": "markup",
    ".toml": "hash",
    ".yaml": "hash",
    ".yml": "hash",
    ".json": "jsonc",
    ".py": "python",
}

REGEX_PRECEDERS = set("(,[{;:!=&|?+-*%<>~^")


class Comment:
    __slots__ = ("start", "end", "text")

    def __init__(self, start, end, text):
        self.start = start
        self.end = end
        self.text = text


def lex_rust(source):
    comments = []
    i = 0
    n = len(source)
    while i < n:
        c = source[i]
        if c == "/" and i + 1 < n and source[i + 1] == "/":
            start = i
            while i < n and source[i] != "\n":
                i += 1
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c == "/" and i + 1 < n and source[i + 1] == "*":
            start = i
            depth = 1
            i += 2
            while i < n and depth:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c == '"':
            i = skip_quoted(source, i, '"', escapes=True)
            continue
        if c == "r" or (c == "b" and i + 1 < n and source[i + 1] == "r"):
            at = i + 1 if c == "b" else i
            j = at + 1
            hashes = 0
            while j < n and source[j] == "#":
                hashes += 1
                j += 1
            if j < n and source[j] == '"':
                i = skip_raw(source, j, hashes)
                continue
            i += 1
            continue
        if c == "'":
            i = skip_char_or_lifetime(source, i)
            continue
        i += 1
    return comments


def skip_quoted(source, i, quote, escapes):
    n = len(source)
    i += 1
    while i < n:
        if escapes and source[i] == "\\":
            i += 2
            continue
        if source[i] == quote:
            return i + 1
        i += 1
    return n


def skip_raw(source, quote_index, hashes):
    closing = '"' + "#" * hashes
    end = source.find(closing, quote_index + 1)
    return len(source) if end < 0 else end + len(closing)


def skip_char_or_lifetime(source, i):
    n = len(source)
    if i + 1 >= n:
        return i + 1
    nxt = source[i + 1]
    if nxt == "\\":
        j = i + 2
        if j < n and source[j] == "u" and j + 1 < n and source[j + 1] == "{":
            end = source.find("}", j)
            return n if end < 0 else end + 2
        if j + 1 < n and source[j + 1] == "'":
            return j + 2
        return i + 1
    if i + 2 < n and source[i + 2] == "'":
        return i + 3
    return i + 1


def lex_c(source):
    comments = []
    i = 0
    n = len(source)
    while i < n:
        c = source[i]
        if c == "/" and i + 1 < n and source[i + 1] == "*":
            start = i
            end = source.find("*/", i + 2)
            i = n if end < 0 else end + 2
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c in "\"'":
            i = skip_quoted(source, i, c, escapes=True)
            continue
        i += 1
    return comments


KEYWORDS_BEFORE_REGEX = {
    "return",
    "typeof",
    "case",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "instanceof",
    "do",
    "else",
    "yield",
    "await",
    "throw",
}


def lex_js(source):
    comments = []
    i = 0
    n = len(source)
    previous = ""
    previous_word = ""
    while i < n:
        c = source[i]
        if c == "/" and i + 1 < n and source[i + 1] == "/":
            start = i
            while i < n and source[i] != "\n":
                i += 1
            comments.append(Comment(start, i, source[start:i]))
            previous = ""
            continue
        if c == "/" and i + 1 < n and source[i + 1] == "*":
            start = i
            end = source.find("*/", i + 2)
            i = n if end < 0 else end + 2
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c in "\"'":
            i = skip_quoted(source, i, c, escapes=True)
            previous = "value"
            previous_word = ""
            continue
        if c == "`":
            i = skip_template(source, i)
            previous = "value"
            previous_word = ""
            continue
        if c == "/":
            if previous == "" or previous in REGEX_PRECEDERS or previous_word in KEYWORDS_BEFORE_REGEX:
                i = skip_regex(source, i)
                previous = "value"
                previous_word = ""
                continue
            previous = "/"
            previous_word = ""
            i += 1
            continue
        if c.isalpha() or c == "_" or c == "$":
            start = i
            while i < n and (source[i].isalnum() or source[i] in "_$"):
                i += 1
            previous_word = source[start:i]
            previous = "value"
            continue
        if not c.isspace():
            previous = "value" if c.isdigit() or c in ")]}" else c
            previous_word = ""
        i += 1
    return comments


def skip_template(source, i):
    n = len(source)
    i += 1
    while i < n:
        if source[i] == "\\":
            i += 2
            continue
        if source[i] == "`":
            return i + 1
        if source.startswith("${", i):
            i = skip_braces(source, i + 2)
            continue
        i += 1
    return n


def skip_braces(source, i):
    n = len(source)
    depth = 1
    while i < n and depth:
        c = source[i]
        if c in "\"'":
            i = skip_quoted(source, i, c, escapes=True)
            continue
        if c == "`":
            i = skip_template(source, i)
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        i += 1
    return i


def skip_regex(source, i):
    n = len(source)
    i += 1
    in_class = False
    while i < n:
        c = source[i]
        if c == "\\":
            i += 2
            continue
        if c == "[":
            in_class = True
        elif c == "]":
            in_class = False
        elif c == "/" and not in_class:
            i += 1
            while i < n and source[i].isalpha():
                i += 1
            return i
        elif c == "\n":
            return i
        i += 1
    return n


def lex_swift(source):
    return lex_c(source)


def lex_shell(source):
    comments = []
    i = 0
    n = len(source)
    if source.startswith("#!"):
        i = source.find("\n")
        if i < 0:
            return comments
    while i < n:
        c = source[i]
        if c == "\n":
            i += 1
            continue
        if c == "#" and (i == 0 or source[i - 1] in " \t\n;&|("):
            start = i
            while i < n and source[i] != "\n":
                i += 1
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c == "'":
            i = skip_quoted(source, i, "'", escapes=False)
            continue
        if c == '"':
            i = skip_shell_double(source, i)
            continue
        if c == "`":
            i = skip_quoted(source, i, "`", escapes=True)
            continue
        if source.startswith("<<", i):
            i = skip_heredoc(source, i)
            continue
        if source.startswith("${", i):
            i = skip_braces(source, i + 2)
            continue
        i += 1
    return comments


def skip_shell_double(source, i):
    n = len(source)
    i += 1
    while i < n:
        if source[i] == "\\":
            i += 2
            continue
        if source[i] == '"':
            return i + 1
        if source.startswith("${", i):
            i = skip_braces(source, i + 2)
            continue
        i += 1
    return n


def skip_heredoc(source, i):
    n = len(source)
    j = i + 2
    if j < n and source[j] == "<":
        return i + 2
    if j < n and source[j] == "-":
        j += 1
    while j < n and source[j] in " \t":
        j += 1
    if j < n and source[j] in "'\"":
        quote = source[j]
        end = source.find(quote, j + 1)
        if end < 0:
            return n
        delimiter = source[j + 1 : end]
        j = end + 1
    else:
        start = j
        while j < n and (source[j].isalnum() or source[j] in "_-"):
            j += 1
        delimiter = source[start:j]
    if not delimiter:
        return i + 2
    line_start = source.find("\n", j)
    if line_start < 0:
        return n
    k = line_start + 1
    while k < n:
        line_end = source.find("\n", k)
        if line_end < 0:
            return n
        if source[k:line_end].strip() == delimiter:
            return line_end
        k = line_end + 1
    return n


def lex_markup(source):
    comments = []
    i = 0
    while True:
        start = source.find("<!--", i)
        if start < 0:
            return comments
        end = source.find("-->", start + 4)
        if end < 0:
            return comments
        comments.append(Comment(start, end + 3, source[start : end + 3]))
        i = end + 3


def lex_hash(source):
    comments = []
    i = 0
    n = len(source)
    while i < n:
        c = source[i]
        if c in "\"'":
            i = skip_quoted(source, i, c, escapes=True)
            continue
        if c == "#":
            start = i
            while i < n and source[i] != "\n":
                i += 1
            comments.append(Comment(start, i, source[start:i]))
            continue
        i += 1
    return comments


def lex_jsonc(source):
    comments = []
    i = 0
    n = len(source)
    while i < n:
        c = source[i]
        if c == "/" and i + 1 < n and source[i + 1] == "/":
            start = i
            while i < n and source[i] != "\n":
                i += 1
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c == "/" and i + 1 < n and source[i + 1] == "*":
            start = i
            end = source.find("*/", i + 2)
            i = n if end < 0 else end + 2
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c == '"':
            i = skip_quoted(source, i, '"', escapes=True)
            continue
        i += 1
    return comments


def lex_python(source):
    comments = []
    i = 0
    n = len(source)
    if source.startswith("#!"):
        i = source.find("\n")
        if i < 0:
            return comments
    while i < n:
        c = source[i]
        if c == "#":
            start = i
            while i < n and source[i] != "\n":
                i += 1
            comments.append(Comment(start, i, source[start:i]))
            continue
        if c in "\"'":
            if source.startswith(c * 3, i):
                end = source.find(c * 3, i + 3)
                i = n if end < 0 else end + 3
            else:
                i = skip_quoted(source, i, c, escapes=True)
            continue
        i += 1
    return comments


LEXERS = {
    "rust": lex_rust,
    "jsonc": lex_jsonc,
    "python": lex_python,
    "js": lex_js,
    "c": lex_c,
    "swift": lex_swift,
    "shell": lex_shell,
    "markup": lex_markup,
    "hash": lex_hash,
}


def tracked_files():
    output = subprocess.run(
        ["git", "ls-files"], capture_output=True, text=True, check=True
    ).stdout
    for path in output.splitlines():
        if not path:
            continue
        if path.startswith(SKIP_PREFIXES):
            continue
        if path.endswith(SKIP_SUFFIXES):
            continue
        name = os.path.basename(path)
        if name in SKIP_NAMES or name.startswith(".git"):
            continue
        language = LANGUAGES.get(os.path.splitext(name)[1])
        if language is None:
            continue
        if "/" not in path and path not in ROOT_FILES:
            continue
        yield path, language


def strip_comments(source, language):
    lexer = LEXERS.get(language)
    if lexer is None:
        return source, 0
    comments = lexer(source)
    if not comments:
        return source, 0
    pieces = []
    cursor = 0
    for comment in comments:
        start = comment.start
        line_start = source.rfind("\n", 0, start) + 1
        if source[line_start:start].strip() == "":
            start = line_start
        pieces.append(source[cursor:start])
        pieces.append("\n" * comment.text.count("\n"))
        cursor = comment.end
    pieces.append(source[cursor:])
    stripped = "".join(pieces)
    stripped = re.sub(r"\n{3,}", "\n\n", stripped)
    stripped = stripped.lstrip("\n")
    if not stripped.endswith("\n"):
        stripped += "\n"
    return stripped, len(comments)


def main(argv):
    mode = argv[1] if len(argv) > 1 else "check"
    if mode in ("-h", "--help"):
        print(USAGE, end="")
        return 0
    if mode not in ("check", "strip"):
        print(USAGE, end="", file=sys.stderr)
        return 2

    total = 0
    files = 0
    for path, language in tracked_files():
        try:
            with open(path, encoding="utf-8") as handle:
                source = handle.read()
        except (UnicodeDecodeError, FileNotFoundError):
            continue
        if mode == "check":
            found = LEXERS[language](source)
            if found:
                files += 1
                total += len(found)
                for comment in found[:3]:
                    line = source.count("\n", 0, comment.start) + 1
                    first = comment.text.splitlines()[0]
                    print(f"{path}:{line}: {first[:100]}")
        else:
            stripped, count = strip_comments(source, language)
            if count and stripped != source:
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write(stripped)
                files += 1
                total += count

    if mode == "check":
        if total:
            print(f"\n{total} comments in {files} files", file=sys.stderr)
            return 1
        print("no comments")
        return 0
    print(f"removed {total} comments from {files} files")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
