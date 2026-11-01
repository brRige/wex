# Wex

### Run Windows applications on Linux. Without the hassle.

**Wex is a free, open-source Windows application manager for Linux, powered by Wine.** It provides a simple graphical interface and command-line tools for installing, launching, and managing Windows `.exe` applications.

Instead of putting every application into one shared Wine environment, Wex gives each application its own isolated environment. This helps keep installations, configurations, and application data separate.

<p align="center">

[**Download Wex**](https://github.com/brRige/wex/releases/latest) · [**Install Wine**](https://www.winehq.org/download) · [**Source Code**](https://github.com/brRige/wex)

</p>

---

## Before you start: Install Wine

Wex uses [Wine](https://www.winehq.org/) to run Windows applications on Linux. **Wine must be installed separately before using Wex.**

Choose the installation method for your Linux distribution:

| Linux distribution | Terminal command |
|---|---|
| Arch Linux | `sudo pacman -S wine` |
| Debian / Ubuntu | `sudo apt install wine` |

For other distributions, or if you want the latest official WineHQ packages, visit:

**[https://www.winehq.org/download](https://www.winehq.org/download)**

After installation, verify that Wine is available:

```bash
wine --version
```

> Note: Package availability and additional 32-bit library requirements vary between distributions. Follow your distribution's Wine installation instructions if additional setup is required.

---

## Features

- **Isolated Wine environments:** Keep applications separated using individual Wine prefixes.
- **Graphical application manager:** Browse and launch your Windows applications through a simple GUI.
- **EXE installation and launching:** Select Windows `.exe` files to install or run them.
- **Application discovery:** Automatically find installed applications through their Start Menu shortcuts.
- **Portable application support:** Add and manage portable Windows programs.
- **Stop applications:** Stop running applications directly from the GUI.
- **Directory access:** View an application's Wine directory and files.
- **Previously launched applications:** Find previously launched `.exe` files through the interface.
- **Error diagnostics:** Understand common Wine errors through simpler explanations.
- **Command-line interface:** Manage and launch applications directly from your terminal.

## Installation

### Option 1: Download the prebuilt release (Recommended)

No Rust installation or compilation required.

**1. Download Wex**

Visit the [Wex GitHub Releases](https://github.com/brRige/wex/releases/latest) page.

Download the Linux x86_64 archive:

`wex-v0.2.0-linux-x86_64.tar.gz`

**2. Extract the archive**

Open a terminal in your Downloads directory:(Or Open terminal  and write "  cd ~/Downloads  " or whatever folder you have downloaded wex in.

```bash
tar -xzf wex-v0.2.0-linux-x86_64.tar.gz
```

**3. Launch Wex**

Enter the extracted directory:

```bash
cd wex-v0.2.0
```

Start the graphical interface:

```bash
./wex-gui
```

To use the command-line version:

```bash
./wex
```

If execution permission is missing, run:

```bash
chmod +x wex wex-gui
```

**Requirements:** Linux x86_64 and Wine installed separately.

### Option 2: Install using Cargo (For developers)

If you have Rust and Cargo installed, you can install Wex directly from its GitHub repository.

Install the GUI version:

```bash
cargo install --git https://github.com/brRige/wex --tag v0.2.0 --features gui
```

Launch the GUI:

```bash
wex-gui
```

To install only the command-line version:

```bash
cargo install --git https://github.com/brRige/wex --tag v0.2.0
```

**Build requirements:** Rust 1.95 or newer, Cargo, and Wine.

---

## Using Wex

### Graphical interface

1. Open Wex using `wex-gui`.
2. Select an application `.exe` file to install or run.
3. Find the application in your Wex library.
4. Click its application tile to launch it.
5. Use the Stop Application button to stop a running application.
6. Check the Output and Crash Logs panels when troubleshooting.

### Command-line interface

Wex can also be used without opening the graphical interface.

```bash
# Install or run a Windows executable
wex your-application.exe

# List installed applications
wex --list

# Launch an installed application
wex notepad
```

---

## How Wex works

Wex uses Wine as its Windows compatibility layer.

When you add an application, Wex manages an isolated Wine prefix for it. A Wine prefix contains the Windows-like environment, configuration, and files used by Wine.

This approach helps prevent one application's installation or configuration from interfering with another.

Wex manages these environments while Wine handles Windows application compatibility.

---

## Compatibility and limitations

Wex is designed for Linux systems running Wine, but not every Windows application is guaranteed to work.

Compatibility depends on factors such as:

- The Windows application and its requirements.
- The installed Wine version.
- Available system libraries and dependencies.
- Graphics and audio drivers.
- Application-specific compatibility issues.

Some applications may require additional components or configuration through Wine.

---

## Requirements

- A Linux system (the prebuilt release currently targets x86_64).
- Wine installed separately.
- A Windows application compatible with Wine.
- Rust 1.95+ and Cargo only if building or installing from source.

---

## Project status

Wex is under active development. Contributions, bug reports, feature requests, and feedback are welcome.

If you encounter an issue, please report it through the [GitHub Issues](https://github.com/brRige/wex/issues) page.

---

## License

Wex is open-source software licensed under the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html).

**Repository:** https://github.com/brRige/wex

**Releases:** https://github.com/brRige/wex/releases
