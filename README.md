# HVNC PoC (Hidden VNC) in Rust

[![Rust](https://img.shields.io/badge/Rust-1.70%2B-blue?logo=rust)](https://www.rust-lang.org/) [![Windows](https://img.shields.io/badge/Platform-Windows-blue?logo=windows)](https://www.microsoft.com/windows)

---

> **Disclaimer:**
> This project is intended **solely for educational and research purposes**. Unauthorized use on systems you do not own or have explicit permission to test is strictly prohibited. The author assumes no liability for any misuse or damage caused by this code.

---

## Overview

**HVNC PoC** (Hidden Virtual Network Computing) is a Rust-based proof-of-concept demonstrating how to create a hidden Windows desktop, launch Google Chrome within it, and capture screenshots of the hidden browser window—all without displaying Chrome to the main user session.

---

## Table of Contents

- [Features](#features)
- [How It Works](#how-it-works)
- [Requirements](#requirements)
- [Installation](#installation)
- [Usage](#usage)
- [Project Structure](#project-structure)
- [Disclaimer](#disclaimer)
- [License](#license)

---

## Features

- 🖥️ **Hidden Desktop:** Creates a hidden Windows desktop using native Win32 APIs
- 🌐 **Chrome Automation:** Launches Google Chrome in the hidden desktop
- 🪟 **Window Enumeration:** Detects and enumerates Chrome windows
- 📸 **Screenshot Capture:** Captures and saves screenshots of Chrome windows

---

## How It Works

1. **Hidden Desktop Creation:** Uses Win32 APIs to create and switch to a hidden desktop.
2. **Chrome Launch:** Starts a new Chrome process in the hidden desktop.
3. **Window Enumeration:** Finds Chrome windows and checks their visibility.
4. **Screenshot Capture:** Captures the window content and saves it as a PNG file.

---

## Requirements

- **Windows 10/11** (x64)
- [Rust toolchain](https://rustup.rs/) (1.70 or newer recommended)
- **Google Chrome** (default path: `C:\Program Files\Google\Chrome\Application\chrome.exe`)
- Rust dependencies (managed by Cargo):
  - [`image`](https://crates.io/crates/image)
  - [`anyhow`](https://crates.io/crates/anyhow)
  - [`chrono`](https://crates.io/crates/chrono)
  - [`windows`](https://crates.io/crates/windows)

---

## Installation

1. **Clone the repository:**
   ```sh
   git clone <this-repo-url>
   cd hidden-vnc
   ```
2. **Edit the Chrome path if needed:**
   Open `src/main.rs` and update the `chrome_path` variable if your Chrome is installed elsewhere.
3. **Build the project:**
   ```sh
   cargo build --release
   ```

---

## Usage

Run the PoC from the project root:

```sh
cargo run --release
```

- Screenshots will be saved in the `screenshots/` directory.
- The program will print status messages to the console, including the location of saved screenshots and any errors encountered.

---

## Project Structure

```
├── src/
│   └── main.rs         # Main PoC logic
├── screenshots/        # Output directory for screenshots
├── Cargo.toml          # Rust dependencies and metadata
└── README.md           # Project documentation
```

---

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.

---

**Author:** Eduardo Contin
**Contact:** <eduardo.contin.04@gmail.com> 
