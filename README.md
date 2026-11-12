# Wex

# SHOWCASE BELOW

### Run Windows applications on Linux. Without the hassle.

**Wex is a free, open-source Windows application manager for Linux, powered by Wine.** It provides a simple graphical interface and command-line tools for installing, launching, and managing Windows `.exe` applications.

Instead of putting every application into one shared Wine environment, Wex gives each application its own isolated environment. This helps keep installations, configurations, and application data separate.

<p align="center">

[**Download Wex**](https://github.com/brRige/wex/releases/latest) · [**Source Code**](https://github.com/brRige/wex)

</p>

---




## Installation (CHECK BELOW FOR VIDEO TUTORIAL)

### Option 1: Download the prebuilt release (Recommended)


**1. Download Wex**

Visit the [Wex GitHub Releases](https://github.com/brRige/wex/releases/latest) page.

Download the Linux x86_64 archive:

`wex-v0.3.0-linux-x86_64.tar.gz`

**2. Extract the archive**

Open a terminal in your Downloads directory:(Or Open terminal  and write "  cd ~/Downloads  " or whatever folder you have downloaded wex in.

```bash
tar -xzf wex-v0.3.0-linux-x86_64.tar.gz
```

# **OR JUST DO "EXTRACT HERE" IN THE GUI IF YOU ARE NOT COMFORTABLE WITH TERMINAL**

**3. Launch Wex**

Enter the extracted directory:

```bash
cd wex-v0.3.0
```

Start the graphical interface:

```bash
./wex-gui
```

# **OR JUST OPEN IT FROM THE  FILE MANAGER GUI IF YOU ARE NOT COMFORTABLE WITH TERMINAL**

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


# **INSTALLATION VIDEO**


https://github.com/user-attachments/assets/20ed12da-2a6b-40b2-9d46-b5d8b926b997


# **SHOWCASE VIDEO**



https://github.com/user-attachments/assets/0e0f3a53-610d-468a-9451-86ab25cb04ab




## License

Wex is open-source software licensed under the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html).

**Repository:** https://github.com/brRige/wex

**Releases:** https://github.com/brRige/wex/releases
