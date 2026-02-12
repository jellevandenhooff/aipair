#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 1 ]; then
    echo "Usage: $0 <test-repo-path>"
    exit 1
fi

TEST_REPO="$(cd "$1" && pwd)"
ENVRC="$TEST_REPO/.envrc"
BEGIN_MARKER="# --- aipair-dev-begin ---"
END_MARKER="# --- aipair-dev-end ---"

if [ ! -f "$ENVRC" ]; then
    echo "No .envrc found in $TEST_REPO"
    exit 0
fi

# Remove marker block
sed -i '' "/$BEGIN_MARKER/,/$END_MARKER/d" "$ENVRC"

# Delete .envrc if empty (only whitespace remaining)
if [ ! -s "$ENVRC" ] || [ -z "$(tr -d '[:space:]' < "$ENVRC")" ]; then
    rm "$ENVRC"
    echo "Removed empty $ENVRC"
else
    echo "Cleaned aipair block from $ENVRC"
fi

# Re-allow direnv if available
if command -v direnv &>/dev/null; then
    direnv allow "$TEST_REPO"
fi
