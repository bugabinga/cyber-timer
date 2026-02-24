# CyberTimer

A blazing fast, cyberpunk-themed terminal timer with a dark aesthetic and neon
pink/cyan accents. Built with Rust and ratatui for maximum performance and
visual impact.

[![CI](https://github.com/bugabinga/cyber-timer/actions/workflows/ci.yml/badge.svg)](https://github.com/bugabinga/cyber-timer/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/cyber-timer)](https://crates.io/crates/cyber-timer)
[![License](https://img.shields.io/github/license/bugabinga/cyber-timer)](LICENSE)

## Features

- ⚡ Blazing fast - built with Rust for maximum performance
- 🎨 Cyberpunk dark theme with neon pink accents
- 🔔 Desktop notifications when timers complete
- 🔊 Audio alerts with fallback sound support
- ⌨️ Keyboard controls (q, Enter, Ctrl+C to quit)
- 📊 Multiple concurrent timers with progress bars
- 🏷️ Named timers support

## Installation

### From Source

```bash
git clone https://github.com/bugabinga/cyber-timer.git
cd cyber-timer
cargo install --path .
```

### Pre-built Binaries

Download the latest release for your platform from the
[releases page](https://github.com/bugabinga/cyber-timer/releases).

## Usage

```
A blazing fast cyberpunk-themed terminal timer

Usage: cyber-timer <TIMERS>...

Arguments:
  <TIMERS>...  Timer durations (e.g., 30s, 5m, 'work:25m')

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### Examples

```bash
# Simple timer (30 seconds)
cyber-timer 30s

# Multiple timers
cyber-timer 5m 2m 30s

# Named timers
cyber-timer "work:25m" "break:5m"

# Mix and match
cyber-timer 30s "coffee:5m" 10m

# Pomodoro!
cyber-timer 25m 5m 25m 5m 25m
```

### Keyboard Controls

- `Enter` or `q` - Exit when all timers complete
- `Ctrl+C` - Exit immediately

## Requirements

- Linux/macOS/Windows
- `notify-send` (Linux) for desktop notifications
- `pw-play` (Linux) for audio alerts, or any audio player

## Configuration

### Custom Sound

Set the `TIMER_SOUND` environment variable to use a custom sound file:

```bash
TIMER_SOUND=/path/to/sound.wav timer 30s
```

## Development

```bash
# Build
cargo build --release

# Run tests
cargo test

# Format
cargo fmt -- --check

# Lint
cargo clippy --all -- -D warnings
```

### Testing with Snapshots

This project uses [insta](https://insta.rs/) for snapshot testing the TUI
rendering. Snapshot tests capture the exact terminal output and detect
unintended visual changes.

```bash
# Run tests (will fail if snapshots don't match)
cargo test

# Review pending snapshot changes
cargo insta review

# Accept all pending snapshot changes
cargo insta accept

# Run tests and automatically accept new snapshots
cargo insta test --accept
```

#### Adding New Snapshot Tests

1. Create a test that uses `TestBackend` to render the widget
2. Use `assert_snapshot!` to capture the output
3. Run the test - it will create a `.snap.new` file
4. Review the snapshot with `cargo insta review`
5. Accept to convert `.snap.new` to `.snap`

Snapshot files are stored in `src/snapshots/`.

## Architecture

```
┌─────────────────────────────────────────┐
│           Cyberpunk Timer              │
├─────────────────────────────────────────┤
│  CLI Args → Parse Duration              │
│         ↓                               │
│  Create Timer States                    │
│         ↓                               │
│  Ratatui TUI Loop                      │
│    ├─ Render Progress Bars              │
│    ├─ Handle Input Events               │
│    └─ Send Notifications                │
└─────────────────────────────────────────┘
```

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file
for details.

## Acknowledgments

- [ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI library
- [crossterm](https://github.com/crossterm-rs/crossterm) - Cross-platform
  terminal library
- [chrono](https://github.com/chronotope/chrono) - Date/time library
