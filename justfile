# aipair development tasks

# Default: show available commands
default:
    @just --list

# Install development dependencies
setup:
    cd web && npm install

# Run the backend server
dev-backend:
    cargo run -- serve

# Run the frontend dev server (hot reload built-in)
dev-frontend:
    cd web && npm run dev

# Run all tests
test:
    cargo test

# Run unit tests only (fast)
test-unit:
    cargo test --lib

# Run integration tests only (builds first to ensure binary exists)
test-integration:
    cargo build
    cargo test --test integration_test

# Build everything (web first, then rust embeds it)
build: build-rust

# Build frontend only
build-web:
    cd web && npm run build

# Build rust binary (release, depends on web assets)
build-rust: build-web
    cargo build --release --features bundled-frontend

# Type check everything
check:
    cargo check
    cd web && npm run typecheck

# Generate TypeScript types from Rust
gen-types:
    cargo test export_bindings_ --features ts-rs/export -- --ignored
    @echo "Types generated in web/src/types/"

# Format code
fmt:
    cargo fmt
    cd web && npx prettier --write src/

# Lint
lint:
    cargo clippy -- -D warnings
    cd web && npm run lint

# Install aipair binary to ~/.local/bin
install: build
    mkdir -p ~/.local/bin
    cp target/release/aipair ~/.local/bin/

# Clean build artifacts
clean:
    cargo clean
    rm -rf web/node_modules web/dist
