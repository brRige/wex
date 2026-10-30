# Wex

**Run Windows applications on Linux. Without the hassle.**

Wex makes it easy to run Windows `.exe` applications on Linux using Wine.

Every application gets its own isolated environment, keeping apps separate and preventing one installation from affecting another. When something goes wrong, Wex explains the problem in simple language.

> **Early Development** · Linux only · Wine must be installed separately.

---

## Installation

1. Download the latest version from [GitHub Releases](https://github.com/brRige/wex/releases).
2. Extract the downloaded `.tar.gz` file.
   
```bash
tar -xvzf wex-v0.1.0-linux-x86_64.tar.gz
```
   
4. Open the extracted folder.
5. Open a terminal in that folder and run:

```bash
./wex-gui
```

That's it! The graphical interface is ready to use.

**Prefer the terminal?** Run applications directly with:

```bash
./wex your-application.exe
```

## Using Wex

- **Add an application:** Select an `.exe` file to install or run it.
- **Launch an application:** Click its tile in the Wex interface.
- **View errors:** Check the Output and Crash Logs panels if something goes wrong.

You can also use Wex from the terminal:

```bash
wex your-application.exe   # Install or run an application
wex --list                 # View installed applications
wex notepad                # Launch an installed application
```

## Requirements

- A Linux system.
- [Wine](https://www.winehq.org/) installed.
- An application compatible with Wine.

**Note:** Not every Windows application or game is guaranteed to work.

## License

Wex is open-source software licensed under [GPL-3.0](https://www.gnu.org/licenses/gpl-3.0.html).

**Source Code:** [github.com/brRige/wex](https://github.com/brRige/wex)
