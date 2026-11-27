# SDL2 Plotter

SDL2 Plotter is a marine navigation application written in Rust. It displays MBTiles charts, vessel data and AIS targets received from a Signal K server, together with routes, tracks and waypoints.
The principles that led me to write this program are:

1/ Optimize code execution using a compiled language (Rust)
2/ To be able to install the code on a desktop-less Linux distribution (such as Dietpi) using the SDL library
3/ To optimize the user interface as much as possible
4/ Enable remote viewing on a tablet via a web page for the cockpit

SDL2 Plotter can run in a desktop Linux environment, but the idea is to create a dedicated, minimalist Linux distribution with rock-solid navigation that's easy to use for everyone at the chart table.


## Features

- Local MBTiles charts
- North Up and Course Up display modes
- Automatic vessel following
- Persistent JSON routes and tracks, with explicit GPX export
- Routes and waypoints
- Distance and bearing measurements, with rhumb lines for route legs
- True or magnetic bearings, with variation from the GPS or the World Magnetic Model
- AIS target display
- Animated zoom
- MJPEG chart streaming over the local network
- Startup configuration through `plotter.toml`

## Requirements

The application builds on Linux (the deployment target, including Raspberry Pi) and on Windows (for development). The Rust code is identical on both; only the way the SDL2 libraries are supplied differs.

### Linux

On Debian, Raspberry Pi OS or Ubuntu, install Rust and the SDL2 libraries:

```bash
sudo apt update
sudo apt install \
  libsdl2-dev \
  libsdl2-ttf-dev \
  libsdl2-image-dev \
  libsdl2-gfx-dev \
  libgbm-dev \
  libegl1-mesa-dev
```

The Signal K server must be reachable from the computer or Raspberry Pi running the application.

### Windows

The step-by-step guide, including what was verified on a real machine, is in [WINDOWS_BUILD.md](WINDOWS_BUILD.md). In short:

1. Install the Visual Studio C++ build tools and Windows SDK (Visual Studio Installer, "Desktop development with C++").
2. Install Rust with the MSVC toolchain: `rustup-init.exe --profile minimal --default-toolchain stable-x86_64-pc-windows-msvc`.
3. Install the SDL2 libraries once with vcpkg: `cargo install cargo-vcpkg` then `cargo vcpkg build` from the repository root.
4. `cargo build`, `cargo test`, `cargo run` as on Linux.

#### How the Windows build works

`Cargo.toml` declares the `sdl2` crate once, with the `ttf`, `gfx` and `unsafe_textures` features, for every platform. A `[target.'cfg(windows)'.dependencies]` table adds two more features on Windows only: `use-vcpkg`, so the build script locates SDL2 in the vcpkg tree under `target/vcpkg`, and `static-link`, so SDL2, SDL2_ttf and SDL2_gfx are linked into `plotter.exe` and no DLLs need to be shipped. A `[package.metadata.vcpkg]` section pins the vcpkg revision and the three ports to build.

Cargo's feature resolver (version 2, the default for edition 2021) ignores target-specific dependencies for platforms that are not being built, so Linux builds resolve `sdl2` with exactly the same three features as before and never see vcpkg. `cargo tree --target x86_64-unknown-linux-gnu -e features -i sdl2-sys` shows this.

#### Alternative approaches on Windows

- **Prebuilt SDL2 development zips.** SDL2 and SDL2_ttf publish `*-devel-*-VC.zip` archives on their GitHub release pages. Point the `LIB` environment variable (or a `.cargo/config.toml` `-L` flag) at the `lib/x64` folders, drop `use-vcpkg` and `static-link`, and copy the DLLs next to the exe. SDL2_gfx has no official Windows binaries, so this only works after replacing the five SDL2_gfx drawing calls with the hand-written primitives already present in `src/screen.rs`. That change would also remove `libsdl2-gfx-dev` from the Linux requirements.
- **GNU toolchain with MSYS2.** Install MSYS2, then `pacman -S mingw-w64-ucrt-x86_64-toolchain mingw-w64-ucrt-x86_64-SDL2 mingw-w64-ucrt-x86_64-SDL2_ttf mingw-w64-ucrt-x86_64-SDL2_gfx`, and use the `stable-x86_64-pc-windows-gnu` Rust toolchain from the UCRT64 shell. This mirrors the apt workflow and needs no Visual Studio, at the cost of a second compiler environment and a GNU-built binary.
- **WSL2.** Follow the Linux instructions inside Ubuntu on WSL2. The SDL window appears through WSLg. Simplest when a Linux-identical build matters more than a native Windows binary.

## Building and running

From the project root:

```bash
cargo run
```

To build an optimized release:

```bash
cargo build --release
./target/release/plotter
```

## Configuration

At first launch, `default/plotter.toml` is created with default values. It can then be edited without recompiling the application.

### Input

```toml
[input]
source = "signalk"   # "signalk", "nmea0183" or "nmea2k" (not supported yet)
```

The app reads live boat data from one source, chosen at start. With no `[input]` table, it reads Signal K. With `nmea2k`, an unknown word, or a bad setting in the chosen source's table (URL, address, talker), it logs an ERROR and runs with no live data: an input setting never stops the start.

### Signal K

```toml
[signalk]
url = "ws://192.168.3.2/signalk/v1/stream?subscribe=none"

[signalk.paths]
position = "navigation.position"
course_over_ground = "navigation.courseOverGroundLastMinute"
speed_over_ground = "navigation.speedOverGroundLastMinute"
heading = "navigation.headingTrue"
magnetic_variation = "navigation.magneticVariation"
datetime = "navigation.datetime"
ais_position = "navigation.position"
ais_name = "name"
ais_course_over_ground = "navigation.courseOverGroundTrue"
ais_speed_over_ground = "navigation.speedOverGround"
ais_ship_type = "design.aisShipType"
ais_nav_state = "navigation.state"
```

The subscription uses these configured paths for vessel data and AIS targets.

### NMEA 0183

```toml
[nmea0183]
transport = "tcp"            # "tcp": connect to address; "udp": bind address and listen
address = "127.0.0.1:10110"  # host:port or IP:port; for UDP, for example "0.0.0.0:10110"
talker = ""                  # position sentences from this talker only, for example "GN"; empty: every talker
```

Used when `source = "nmea0183"`. To test with `go-gps-simulator`: run `gps-simulator -tcp` with `transport = "tcp"`, `address = "127.0.0.1:10110"`, or `gps-simulator -udp -host 127.0.0.1` with `transport = "udp"`, `address = "0.0.0.0:10110"`. The app reads RMC (position, speed, course), HDT and HDG (heading). An HDG with no variation of its own is made true with the variation from RMC, else from the magnetic model. When no RMC comes for 5 s, it uses GGA or GLL for the position and VTG for speed and course. It also reads AIS targets from `!AIVDM` (message types 1, 2, 3, 5, 18, 19 and 24).

### Charts

```toml
[charts]
directory = "map"
default = "assets/OSM.mbtiles"
```

Available charts are `.mbtiles` files stored in `directory`. The current chart selection is saved in `default/state.json`.

### Window

```toml
[window]
title = "KALYMNOS"
fullscreen = false
width = 1600
height = 800
```

### Appearance

Symbol sizes are expressed in pixels. Colors use `[red, green, blue, alpha]`, with values between 0 and 255.

```toml
[appearance]
boat_size = 9                 # round marker radius, when no direction is known
boat_circle_radius = 50       # heading line length
boat_hull_size = 27           # own-boat hull, centre to bow
boat_color = [255, 0, 0, 255]
boat_no_fix_color = [128, 128, 128, 255]   # no position for 10 s
mark_size = 6
measure_color = [0, 0, 255, 255]
route_width = 3
route_color = [255, 0, 0, 255]
track_color = [255, 0, 0, 255]
waypoint_color = [220, 30, 30, 255]
coverage_color = [255, 140, 0, 255]
ais_cargo_color = [0, 125, 0, 255]        # ship types 70-79
ais_tanker_color = [150, 0, 150, 255]     # 80-89
ais_passenger_color = [0, 70, 200, 255]  # 60-69
ais_high_speed_color = [185, 150, 0, 255] # 40-49
ais_service_color = [0, 130, 140, 255]    # 50-59: pilot, SAR, tug, law
ais_fishing_color = [215, 85, 0, 255]    # 30
ais_leisure_color = [105, 45, 175, 255]  # 36-37: sailing, pleasure
ais_other_color = [95, 95, 95, 255]    # any other, or not known yet
```

### Rendering

```toml
[rendering]
tile_cache_size = 512
prefetch_margin = 2
```

### Routes and tracks

```toml
[track]
checkpoint_interval_seconds = 15
```

Routes and the current track are stored as versioned JSON in `default/route.json` and `default/track.json`. The track is checkpointed periodically. GPX exports for routes, tracks and waypoints are available from their respective menus and written to `export/`.

On first launch after upgrading, an existing `default/route.gpx` or `default/track.gpx` is migrated to its corresponding JSON file. The original GPX file is left unchanged.

### Bearings and magnetic variation

Route legs, and the bearing from the boat to the next waypoint, are rhumb lines: a constant bearing that a helmsman can hold. The measure tool shows the great circle, and adds the rhumb line ("RL") when the two differ by more than 0.5 degrees or 0.1 %.

The bottom bar shows `COG 057°T  HDG 095°T`. The COG shows only with a fix and above 1 m/s (about 2 kn); a missing value shows as `--`. Each bearing has a T (true) or M (magnetic) mark. "Orientation > Magnetic bearings" switches own-boat bearings to magnetic; it is saved in `state.json`. AIS course and heading stay true. While it is on, the bottom bar shows the variation in use and its source:

- `GPS`: any live variation (from RMC, HDG or the Signal K `magnetic_variation` path), at most 10 s old.
- `WMM`: the World Magnetic Model WMM2025, built into the app, at the boat position and the GPS date (the system date with no fix).
- `WMM expired`: the same model after 2029. NOAA publishes a new model every five years.
- `Var --`: near a magnetic pole, where the model is not reliable. Bearings stay true.

Deviation is not applied: a magnetic bearing is not a compass bearing.

Notice under 17 U.S.C. 403: the WMM2025 coefficients (`src/magnetic/WMM.COF`) are a work of the US Government (NOAA National Centers for Environmental Information) and are not subject to copyright protection in the United States.

### Collision alarm

When enabled, the application predicts the closest point of approach (CPA) for
AIS targets using the boat and target course/speed. It shows a red banner and
highlights the target when both thresholds are reached, and emits a short
generated beep when a new risk appears. The alarm remains active while the
risk remains present.

```toml
[collision]
enabled = true
cpa_nm = 0.5
tcpa_minutes = 15.0
proximity_nm = 2.0
```

`proximity_nm` starts a separate, orange alarm when a target is this close and
sends no valid SOG (speed over ground) or COG (course over ground).

### Network streaming

The MJPEG stream can be viewed in a browser on a local computer or tablet:

```toml
[streaming]
enabled = true
bind_address = "0.0.0.0"
port = 8080
fps = 5
jpeg_quality = 70
```

With `bind_address = "0.0.0.0"`, the server listens on every network interface. From a tablet, open:

```text
http://RPI_ADDRESS:8080/
```

The direct MJPEG stream is available at:

```text
http://RPI_ADDRESS:8080/stream
```

`bind_address` must be the real IP address of the machine running the application, or `0.0.0.0` to listen on all interfaces. A network address such as `192.168.3.0` does not identify a host. Use the machine firewall to restrict access.

If the address cannot be bound (for example the boat's address on a development PC), the application logs an ERROR in `plotter.log` and runs without the stream; the chart is never held back by the tablet feed. On a development machine set `enabled = false` or `bind_address = "127.0.0.1"` to avoid the error.

## Runtime files

Only `default/plotter.toml` is part of the repository. Everything else below is written by the application, belongs to the installation it runs on, and is ignored by git, so a fresh clone starts with an empty route, track and waypoint list and the position saved in the chart's own centre.

- `default/plotter.toml`: configuration editable without recompiling
- `default/state.json`: position, zoom and dynamic preferences
- `default/route.json`: current route
- `default/waypoints.json`: waypoint list
- `default/track.json`: current track
- `default/route.gpx` and `default/track.gpx`: legacy files retained after migration
- `export/`: timestamped GPX route, track and waypoint exports
- `plotter.log`: log and diagnostics. INFO and above are written by default; set the environment variable `PLOTTER_LOG=debug` to include every input event, or `warn` to keep only problems. The file is capped at 5 MB and rotated once to `plotter.log.1`, so the log never takes more than 10 MB of the SD card.

## Architecture

- `src/app.rs`: main loop and orchestration
- `src/app/actions.rs`: application actions
- `src/app/input.rs`: keyboard and mouse input
- `src/app/render.rs`: chart and overlay assembly
- `src/mbtiles_renderer.rs`: projection, tiles, zoom and cache
- `src/signalk.rs`: Signal K connection and decoding
- `src/nmea0183.rs`, `src/nmea0183/transport.rs`: NMEA 0183 decoding, TCP and UDP input
- `src/feed.rs`: the channels every input source publishes on
- `src/screen.rs`: SDL2 rendering
- `src/stream.rs`: network MJPEG server
- `src/config.rs`: `plotter.toml` loading and validation

## Tests

```bash
cargo test
```
