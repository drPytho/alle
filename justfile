set dotenv-load

# Default recipe to display help information
default:
    @just --list

# Build the project
build:
    cargo build

# Build the project in release mode
build-release:
    cargo build --release

# Run the binary
run *ARGS:
    cargo run -- {{ARGS}}

# Run the binary in release mode
run-release *ARGS:
    cargo run --release -- {{ARGS}}

# Run all tests
test:
    cargo test

# Run tests with output
test-verbose:
    cargo test -- --nocapture

# Run only unit tests
test-unit:
    cargo test --lib postgres::tests::unit_tests

# Run only integration tests
test-integration:
    cargo test --lib postgres::tests::integration_tests

# Run tests with a specific database URL
test-db URL:
    TEST_DATABASE_URL={{URL}} cargo test

# Check the project for errors
check:
    cargo check

# Check all targets
check-all:
    cargo check --all-targets

# Run clippy linter
clippy:
    cargo clippy -- -D warnings

# Run clippy with all features
clippy-all:
    cargo clippy --all-targets --all-features -- -D warnings

# Format the code
fmt:
    cargo fmt

# Check formatting without making changes
fmt-check:
    cargo fmt -- --check

# Run all quality checks (fmt, clippy, test)
ci: fmt-check clippy test

# Clean build artifacts
clean:
    cargo clean

# Watch for changes and run tests
watch-test:
    cargo watch -x test

# Watch for changes and run the binary
watch-run *ARGS:
    cargo watch -x "run -- {{ARGS}}"

# Show project dependencies
deps:
    cargo tree

# Update dependencies
update:
    cargo update

# Run the binary with RUST_LOG=debug
debug *ARGS:
    RUST_LOG=debug cargo run -- {{ARGS}}

# Run the binary with RUST_LOG=trace
trace *ARGS:
    RUST_LOG=trace cargo run -- {{ARGS}}

# Generate documentation
doc:
    cargo doc --no-deps --open

# Run security audit
audit:
    cargo audit

# Benchmark the project (if benchmarks exist)
bench:
    cargo bench
