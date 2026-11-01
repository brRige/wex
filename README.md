# Wex

**Run Windows applications on Linux. Without the hassle.**

Wex makes it easy to run Windows `.exe` applications on Linux using Wine.

Every application gets its own isolated environment, keeping apps separate and preventing one installation from affecting another. When something goes wrong, Wex explains the problem in simple language.


---

## Installation

### Option A: Download (Recommended for Regular Users)

No building or complicated setup required!

**1. Download Wex**

Visit the [Wex GitHub Releases](https://github.com/brRige/wex/releases) page.

Open the latest release and download:

`wex-v0.1.0-linux-x86_64.tar.gz`

**2. Extract the archive**

Open your Downloads folder in the terminal and run:

```bash
tar -xvzf wex-v0.1.0-linux-x86_64.tar.gz
```

**3. Open Wex**

Navigate to the folder containing the extracted files and run:

```bash
./wex-gui
```

If Linux doesn't allow you to execute it, run:

```bash
chmod +x wex-gui wex
```

Then launch it again:

```bash
./wex-gui
```
You should be Good To Go now

### Option B: Build from Source (For Developers)

If you prefer installing Wex through Rust's Cargo package manager, make sure you have Rust, Cargo and Wine installed.

Run:

```bash
cargo install --git https://github.com/brRige/wex --features gui
```

Once installation finishes, launch Wex:

```bash
wex-gui
```

To install without the graphical interface, omit `--features gui`.

---

## Using Wex

- **Add an application:** Select an `.exe` file to install or run it.
- **Launch an application:** Click its tile in the Wex interface.
- **View errors:** Check the Output and Crash Logs panels if something goes wrong.

### Terminal Commands

You can also use Wex directly from your terminal:

```bash
wex your-application.exe   # Install or run an application
wex --list                 # View installed applications
wex notepad                # Launch an installed application
```

---

## Requirements

- A Linux system.
- [Wine](https://www.winehq.org/) installed.
- An application compatible with Wine.

**Note:** Not every Windows application or game is guaranteed to work.

---

## License

Wex is open-source software licensed under [GPL-3.0](https://www.gnu.org/licenses/gpl-3.0.html).

**Source Code:** [github.com/brRige/wex](https://github.com/brRige/wex)
