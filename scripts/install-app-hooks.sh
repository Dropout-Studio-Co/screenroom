#!/bin/bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "install-app-hooks: macOS only; nothing to do on $(uname -s)"
  exit 0
fi

hooks="$root/scripts/git-hooks"
for hook in pre-commit post-commit post-merge post-rewrite; do
  if [ ! -x "$hooks/$hook" ]; then
    chmod +x "$hooks/$hook"
  fi
done

current="$(git config --get core.hooksPath || true)"
git config core.hooksPath scripts/git-hooks

mkdir -p .artifacts/builds

echo "core.hooksPath: ${current:-<unset>} -> scripts/git-hooks"
echo "hooks installed: pre-commit, post-commit, post-merge, post-rewrite"
echo "check with: scripts/rebuild-app.sh --status"
echo "build now:  scripts/rebuild-app.sh --force"
