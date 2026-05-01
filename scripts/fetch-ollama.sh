#!/usr/bin/env bash
# Populate src-tauri/binaries/ with the Ollama binary for Tauri's sidecar bundle.
# Tauri requires binaries to be named with the target triple suffix.
#
# Source: the binary shipped inside /Applications/Ollama.app (universal, x86_64+arm64).
# If Ollama isn't installed, install it first: `brew install --cask ollama` or
# download from https://ollama.com/download.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO_ROOT/src-tauri/binaries"
SRC="/Applications/Ollama.app/Contents/Resources/ollama"

if [[ ! -f "$SRC" ]]; then
  echo "error: $SRC not found. Install Ollama first." >&2
  exit 1
fi

mkdir -p "$DEST"
cp "$SRC" "$DEST/ollama-aarch64-apple-darwin"
cp "$SRC" "$DEST/ollama-x86_64-apple-darwin"
chmod +x "$DEST"/ollama-*-apple-darwin

echo "wrote sidecar binaries:"
ls -lh "$DEST"
