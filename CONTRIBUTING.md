# Contributing to Cyberpunk Timer

Thank you for your interest in contributing!

## Development Process

1. Fork the repo
2. Create a feature branch
3. Make your changes
4. Run tests and lints
5. Submit a PR

## Getting Started

```bash
# Clone your fork
git clone https://github.com/YOUR_USERNAME/timer.git
cd timer

# Install dependencies
cargo fetch

# Build
cargo build --release

# Run
cargo run -- 30s 5m
```

## Code Style

We use `rustfmt` for code formatting:

```bash
cargo fmt --all
```

We use `clippy` for linting:

```bash
cargo clippy --all -- -D warnings
```

## Testing

```bash
cargo test --all
```

## Commit Messages

- Use imperative mood ("Add feature" not "Added feature")
- Keep the first line under 72 characters
- Reference issues and PRs in the body

## PR Requirements

- [ ] Tests pass
- [ ] Code is formatted
- [ ] No clippy warnings
- [ ] Documentation updated if needed

## License

By contributing, you agree that your contributions will be licensed under the
MIT License.
