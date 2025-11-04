# vvdaw justfile
# Run `just` to see available commands

# Default command: show available recipes
default:
    @just --list

# Build all crates in the workspace
build:
    cargo build

# Build with optimizations
build-release:
    cargo build --release

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run clippy lints (strict mode with -D warnings)
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Check code without building
check:
    cargo check --workspace --all-targets --all-features

# Format all code
fmt:
    cargo fmt --all

# Check if code is formatted
fmt-check:
    cargo fmt --all -- --check

# Run all pre-commit checks (format, lint, test)
pre-commit: fmt lint test

# Run all CI checks (what GitHub Actions runs)
ci: fmt-check lint test build-release

# Clean build artifacts
clean:
    cargo clean

# Update dependencies
update:
    cargo update

# Run the main binary with args (e.g., `just run -- --help`)
run *ARGS:
    cargo run --bin vvdaw -- {{ARGS}}

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open

# Check for outdated dependencies
outdated:
    cargo outdated

# Audit dependencies for security vulnerabilities
audit:
    cargo audit

# Fix clippy warnings automatically where possible
fix:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty

# Fix formatting
fix-fmt:
    cargo fmt --all

# Run all fixes
fix-all: fix-fmt fix

# Watch and run tests on file changes (requires cargo-watch)
watch:
    cargo watch -x test

# Watch and run CI checks on file changes (requires cargo-watch)
watch-ci:
    cargo watch -x check -x test -x clippy
