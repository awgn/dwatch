# Dwatch

`dwatch` is a modern, Rust-based replacement for the traditional Unix `watch` tool. It provides advanced monitoring capabilities, multiple visualization styles with delta tracking, interactive controls, and flexible command execution.

## Table of Contents

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- [Command-Line Options](#command-line-options)
- [Visualization Styles](#visualization-styles)
- [Interactive Mode](#interactive-mode)
- [Configuration & Persistence](#configuration--persistence)
- [Examples](#examples)
- [License](#license)

## Features

- **Dynamic Value Tracking:** Tracks numeric values in command output across intervals to calculate deltas, rates, and bandwidth.
- **Multiple Styles:** View data as raw numbers, changes per interval, events per second, engineering notation, or network bandwidth (bps).
- **Interactive Focus:** Select specific values on the screen and independently change their visualization style without affecting the rest of the output.
- **Multiple Commands:** Execute and monitor multiple separate commands simultaneously in the same terminal.
- **Persistent State:** Your chosen visualization styles for specific commands and focused values are saved automatically and restored on the next run.

## Installation

Ensure you have Rust and Cargo installed, then clone the repository and build:

```bash
git clone https://github.com/nicola/dwatch.git
cd dwatch
cargo build --release
cargo install --path .
```

## Usage

DWatch executes a command repeatedly at a specified interval, highlighting numeric values and supporting real-time rate calculations.

### Basic Usage

```bash
dwatch [OPTIONS] <COMMAND>...
```

### With Multiple Commands

```bash
dwatch -m "ls -l" "free -m" "uptime"
```

## Command-Line Options

| Option | Short | Description |
| :--- | :--- | :--- |
| `--seconds <SECONDS>` | `-s` | Exit automatically after the specified number of seconds. |
| `--interval <INTERVAL>` | `-i` | Set the update interval in seconds (default: 1). |
| `--multiple-commands` | `-m` | Interpret the remaining arguments as multiple separate commands instead of arguments to a single command. |
| `--style <STYLE>` | | Set the initial global visualization style. See [Visualization Styles](#visualization-styles) for options. |
| `--no-banner` | `-n` | Suppress the header banner that shows the interval, current style, and commands being executed. |

## Visualization Styles

DWatch can automatically calculate and format changes in numeric output over time. The following styles are supported:

1. **`default`**: Displays the numeric value in bold blue.
2. **`number+(events per interval)`**: Shows the numeric value in bold red, followed by the absolute change (Δ) since the last interval (e.g., `42⟶5/i`).
3. **`number+(events per second)`**: Shows the numeric value in bold red, followed by the calculated rate of change per second (e.g., `42⟶2.5/s`).
4. **`events per interval`**: Displays only the delta per interval in bold red (e.g., `5/i`).
5. **`events per second`**: Displays only the rate of change per second in bold red (e.g., `2.50/s`).
6. **`engineering`**: Shows the numeric value in bold purple, followed by the rate of change per second using engineering suffixes (K, M, G).
7. **`networking`**: Shows the numeric value in bold green, followed by the rate of change converted to bits per second (bps) with unit scaling (e.g., `1.50Mbps`).

*Note: You can specify the style by its exact name using the `--style` argument.*

## Interactive Mode

DWatch provides real-time keyboard controls to tweak your monitoring view on the fly.

- **`Ctrl+Z` (SIGTSTP)**: **Move Focus**. Cycles focus through all numeric values found in the output. When a value is focused, it is highlighted in reverse video. If no action is taken for 5 intervals, focus is automatically cleared.
- **`Ctrl+\` (SIGQUIT)**: **Change Style**. Cycles through the available visualization styles. If a specific number is focused, only that number's style will change. If no number is focused, it changes the global default style.

## Configuration & Persistence

DWatch remembers your visualization preferences! When you assign a specific style to a specific number in a command's output, DWatch saves this map.

- **Config Location:** `~/.config/dwatch/styles.json`
- **Format:** NDJSON (Newline Delimited JSON).
- **Behavior:** Settings are saved on graceful exit (e.g., `Ctrl+C`) and automatically loaded the next time you monitor the exact same command.

## Examples

**Monitor system load**
```bash
dwatch "uptime"
```

**Monitor a network interface (ideal with the networking style)**
```bash
dwatch --style networking -i 2 "cat /proc/net/dev | grep eth0"
```

**Monitor multiple system metrics concurrently**
```bash
dwatch -m "free -h" "df -h /" "uptime"
```

**Run for a limited time without the banner**
```bash
dwatch -s 60 -n "sensors"
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contact

Nicola Bonelli - [nicola@larthia.com](mailto:nicola@larthia.com)
