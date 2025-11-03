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

test_one *ARGS:
    cargo test -- {{ARGS}}

test_integration: test_auth test_e2e

test_auth:
    cargo test --test sse_auth_tests -- --ignored

test_e2e:
    cargo test --test sse_postgres_e2e_tests -- --ignored

check:
    cargo check --all-targets

# Run clippy linter
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Format the code
fmt:
    cargo fmt

# Check formatting without making changes
fmt-check:
    cargo fmt -- --check

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

# Generate documentation
doc:
    cargo doc --no-deps --open

# Run security audit
audit:
    cargo audit
