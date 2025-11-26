#!/bin/bash
set -e

cd "$(dirname "$0")/.."

echo "Building frontend..."
cd web
npm ci
npm run build
cd ..

echo "Building release binary with bundled frontend..."
cargo build --release --features bundled-frontend

echo ""
echo "Done! Binary at: target/release/aipair"
echo ""
echo "To install globally: cargo install --path . --features bundled-frontend"
