# Building Folio

Tested on Ubuntu. Setup commands for Ubuntu, Debian, Fedora, Arch, and openSUSE are below.

## Requirements

- Rust **1.92 or newer**, including Cargo. Use your distribution's packages if recent enough, or follow the official [Rust installation instructions](https://www.rust-lang.org/tools/install) for rustup.
- A C/C++ build toolchain, `pkg-config`, and development libraries for X11, Wayland, OpenGL/EGL, and xkbcommon.
- Python 3 and GNU coreutils (`install -D`) for the installer.
- An X11 or Wayland desktop with OpenGL 2.0+/OpenGL ES 2.0 support to run the app. Software rendering is also possible.

SQLite and the fonts are bundled; no SQLite server or separate font installation is needed. The commands below install system prerequisites, **not Rust**. Check your toolchain separately:

```sh
rustc --version
cargo --version
```

## System prerequisites

Run the commands for your distribution, then continue to [build and install](#build-run-or-install).

### Ubuntu / Debian family

If your release ships Rust below 1.92, install a current version with rustup.

```sh
sudo apt update
sudo apt install build-essential pkg-config python3 coreutils \
  libx11-dev libxi-dev libxrandr-dev libxcursor-dev libxinerama-dev \
  libgl-dev libegl-dev libxkbcommon-dev libxkbcommon-x11-0 \
  libwayland-dev libgl1-mesa-dri
```

### Fedora

```sh
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config python3 coreutils \
  libX11-devel libXi-devel libXrandr-devel libXcursor-devel libXinerama-devel \
  libglvnd-devel libxkbcommon-devel libxkbcommon-x11 wayland-devel
```

### Arch Linux

Use a fully updated system; `-Syu` updates the system as well as installing these packages.

```sh
sudo pacman -Syu --needed base-devel python coreutils \
  libx11 libxi libxrandr libxcursor libxinerama libglvnd \
  libxkbcommon libxkbcommon-x11 wayland mesa
```

### openSUSE

```sh
sudo zypper install gcc gcc-c++ make pkg-config python3 coreutils \
  libX11-devel libXi-devel libXrandr-devel libXcursor-devel libXinerama-devel \
  Mesa-libGL-devel Mesa-libEGL-devel libxkbcommon-devel libxkbcommon-x11-0 \
  wayland-devel
```

For other distributions, install the equivalent packages with your package manager.

## Build, run, or install

Download and extract the source, then open a terminal in the Folio folder. To try it without installing:

```sh
cargo run --release --locked
```

`--release` enables optimization; `--locked` keeps the dependency versions in `Cargo.lock`. The first build can take several minutes and download dependencies. To build without launching, use `cargo build --release --locked`; the executable is `target/release/folio`.

For a per-user installation instead:

```sh
sh scripts/install-local.sh
"$HOME/.local/bin/folio"
```

The installer adds Folio to your application menu. It puts the executable in `~/.local/bin/` and the icon and documentation in `~/.local/share/`. Close Folio before reinstalling. If you have customized Cargo's output directory, use the default `target/` directory for this installer.

If the menu entry does not appear immediately, launch with the full path above. If `folio` alone is not found, add `~/.local/bin` to your shell's `PATH` or keep using the full path.

## Troubleshooting

- **Rust is too old:** install a current stable toolchain and check for an older project-specific rustup override.
- **A library or `.pc` file is missing:** install the corresponding development package for your distribution. Package names can change between releases.
- **No display / graphics initialization failed:** run in a graphical desktop session and check your graphics drivers. With Mesa, `LIBGL_ALWAYS_SOFTWARE=1 cargo run --release --locked` can help diagnose GPU/driver issues.
- **`GLIBC_* not found` after copying an executable:** rebuild from source on the destination system. A binary built on a newer distribution is not necessarily compatible with an older one, even on the same CPU architecture.

## For contributors

Run the checks with:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

See [architecture notes](ARCHITECTURE.md) for how Folio works.
