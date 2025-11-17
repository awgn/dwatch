# Dwatch

DWatch is a modern replacement for the `watch` Unix tool, implemented in Rust. It provides advanced monitoring capabilities with multiple visualization styles, interactive control, and flexible command execution.

## Table of Contents

- [Dwatch](#dwatch)
  - [Table of Contents](#table-of-contents)
  - [Getting Started](#getting-started)
  - [Usage](#usage)
  - [Command-Line Options](#command-line-options)
  - [Visualization Styles](#visualization-styles)
  - [Interactive Mode](#interactive-mode)
  - [Configuration](#configuration)
  - [Examples](#examples)
  - [License](#license)
  - [Contact](#contact)

## Getting Started

To get a local copy up and running, follow these steps:

1. Clone the repository
   ```
   git clone https://github.com/your-username/dwatch.git
   ```

2. Navigate to the project directory
   ```
   cd dwatch
   ```

3. Run the following commands to compile and install the program:

   - Compile the project
     ```
     cargo build --release
     ```

   - Install the program
     ```
     cargo install --path .
     ```

## Usage

DWatch executes a command repeatedly at a specified interval and displays the output with enhanced highlighting and formatting of numeric values.

### Basic Usage

```bash
dwatch [OPTIONS] <COMMAND>
```

### With Multiple Commands

```bash
dwatch -m <COMMAND1> <COMMAND2> <COMMAND3>
```

## Command-Line Options

- **`-s, --seconds <SECONDS>`**: Exit after the specified number of seconds (optional). If not specified, DWatch runs indefinitely.

- **`-n, --no-banner`**: Suppress the banner that shows the interval, current style, and commands being executed.

- **`-m, --multiple-commands`**: Interpret the remaining arguments as multiple separate commands instead of a single command. Each command will be executed independently and its output displayed.

- **`-i, --interval <INTERVAL>`**: Set the update interval in seconds (default: 1 second). Determines how often the command is executed and the output is refreshed.

- **`--style <STYLE>`**: Set the visualization style for displaying numeric values. See [Visualization Styles](#visualization-styles) for available options.

## Visualization Styles

DWatch supports multiple visualization styles for displaying numeric values extracted from command output. Each style highlights values and can show delta (change) information:

### Available Styles

1. **`default`**: Displays the numeric value in bold blue. This is the standard mode.

2. **`number+(events per interval)`**: Shows the numeric value in bold red, followed by the delta (Δ) per interval (e.g., `42⟶5/i` means value is 42 and changed by 5 in this interval).

3. **`number+(events per second)`**: Shows the numeric value in bold red, followed by the rate of change per second (e.g., `42⟶2.5/s` means value is 42 and changed by 2.5 per second).

4. **`events per interval`**: Displays only the delta per interval in bold red (e.g., `5/i`).

5. **`events per second`**: Displays only the rate of change per second in bold red with automatic unit scaling (e.g., `2.50/s`).

6. **`engineering`**: Shows the numeric value in bold purple, followed by the rate of change per second with engineering notation (K, M, G suffixes). Useful for large numbers.

7. **`networking`**: Shows the numeric value in bold green, followed by the rate of change in bits per second (bps) with unit scaling (e.g., `1.50Mbps`). Ideal for monitoring network bandwidth.

### Selecting a Style

Use the `--style` option to set the initial style:

```bash
dwatch --style networking "cat /proc/net/dev | grep eth0"
```

## Interactive Mode

DWatch provides interactive controls during execution to dynamically switch between styles and manage focus on specific numeric values.

### Interactive Keyboard Shortcuts

- **`Ctrl+\` (SIGQUIT)**: Cycle to the next visualization style for the currently focused numeric value, or globally if no value is focused. This allows you to switch between different visualization modes without interrupting the monitoring.

- **`Ctrl+Z` (SIGTSTP)**: Move focus to the next numeric value in the output. Focus cycles through all numeric values and can be released. When a value has focus, it can have its own independent style different from the global style. After 5 refreshes without focus action, the focus is automatically released.

### Focus Behavior

- When you move focus to a numeric value with `Ctrl+Z`, that value is highlighted with bold and reverse video.
- Each focused value can have its own independent visualization style, allowing you to monitor different metrics with different display modes simultaneously.
- Press `Ctrl+Z` repeatedly to cycle through all numeric values in the output.
- The focus display shows `(focus:N)` in the banner to indicate which numeric value (index) is currently focused.

## Configuration

DWatch stores style preferences in a configuration file to maintain your preferred settings across sessions.

### Configuration File Location

Style preferences are saved in:
```
~/.config/dwatch/styles.json
```

### Configuration Format

The configuration file uses NDJSON (newline-delimited JSON) format. Each line represents a command and its style preferences:

```json
{"command":"command1","styles":{}}
{"command":"command2","styles":{"0":2,"5":1}}
```

Where:
- `command`: The full command string being monitored
- `styles`: A map of numeric value indices to their style indices (0=default, 1=number+events/interval, etc.)

The configuration is automatically loaded when you start monitoring a command, and updated when you exit (by pressing `Ctrl+C` or `Ctrl+D`).

## Examples

### Monitor system load
```bash
dwatch "uptime"
```

### Monitor network interface with 2-second interval
```bash
dwatch -i 2 "cat /proc/net/dev | grep eth0"
```

### Monitor multiple commands with networking style
```bash
dwatch -m --style networking "cat /proc/net/dev" "ss -s"
```

### Monitor for 30 seconds with no banner
```bash
dwatch -s 30 -n "free -h"
```

### Monitor disk usage with engineering notation
```bash
dwatch --style engineering "df -h | tail -n +2"
```

### Monitor network bandwidth
```bash
dwatch --style networking --interval 1 "cat /proc/net/dev | grep eth0"
```

Then use `Ctrl+\` to switch between "networking", "events per second", and other styles in real-time.

### Monitor multiple metrics simultaneously
```bash
dwatch -m "cat /proc/meminfo | head -5" "cat /proc/loadavg"
```

Then use `Ctrl+Z` to focus on specific values and `Ctrl+\` to switch their individual styles.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contact

If you have any questions or suggestions, feel free to reach out to the project author:

Nicola Bonelli - [nicola@larthia.com](mailto:nicola@larthia.com)