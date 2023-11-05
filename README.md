# Dwatch

DWatch is a modern replacement for the `watch` Unix tool, implemented in Rust.

## Table of Contents

- [Dwatch](#dwatch)
  - [Table of Contents](#table-of-contents)
  - [Getting Started](#getting-started)
  - [Usage](#usage)
  - [Visualization Modes](#visualization-modes)
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

DWatch accepts the following command-line arguments:

- `--seconds` or `-s`: Exit after the specified number of seconds (optional)
- `--no-banner` or `-n`: Suppress the banner
-  `--multiple-commands` or `-m`: Interpret arguments as multiple commands
-  `--interval` or `-i`:  Set the update interval in seconds

## Visualization Modes

- **Default Mode**: Displays the numeric value in blue.

- **Delta Mode**: Shows the numeric value in blue, along with the delta (Δ) in red if it's not zero.

- **Delta Highlight Mode**: Highlights the delta in red.

- **Delta Rate Mode**: Calculates the delta rate and displays it in red as rate. This mode is useful when tracking changes over time.

- **Delta Rate Highlight Mode**: Calculates the delta rate and displays it in green (as bit per second). It's a variant of the Delta Rate Mode with green highlighting.

You can switch between these modes interactively by using the `Ctrl+\` shortcut during runtime. This allows you to adapt the visualization to your specific monitoring needs.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contact

If you have any questions or suggestions, feel free to reach out to the project author:

Nicola Bonelli - [nicola@larthia.com](mailto:nicola@larthia.com)
