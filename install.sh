#!/usr/bin/env bash
# Build and install Timbits for the current user.
set -euo pipefail

cd "$(dirname "$0")"

echo "==> Building timbits (release)…"
cargo build --release

BIN_DIR="$HOME/.local/bin"
mkdir -p "$BIN_DIR"
cp target/release/timbits "$BIN_DIR/timbits"
echo "==> Installed binary to $BIN_DIR/timbits"

echo "==> Setting up config, autostart and launcher entries…"
"$BIN_DIR/timbits" install

if ! command -v tesseract >/dev/null 2>&1; then
    echo
    echo "NOTE: tesseract not found — image OCR in history search is disabled."
    echo "      Enable it with: sudo apt install tesseract-ocr"
fi

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo; echo "NOTE: $BIN_DIR is not on your PATH — add it to your shell profile." ;;
esac

echo
echo "🍩 Done! Log out/in (or run 'timbits daemon &' now) to start the watcher."
