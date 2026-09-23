# BlueMIDI

Low-latency Bluetooth Low Energy (BLE) to MIDI bridge for Windows with full MIDI Polyphonic Expression (MPE) support.

BlueMIDI runs as an unobtrusive background process in the Windows notification area (System Tray). It connects to BLE-MIDI instruments—including ROLI Piano M, LUMI Keys, Seaboard Block / RISE, and generic BLE-MIDI controllers—and forwards MIDI events to a kernel-level virtual MIDI port with sub-millisecond dispatch and optimized connection intervals.

## Features

- **Optimized BLE Connection Interval**: Requests `ThroughputOptimized` connection parameters (7.5 ms – 15 ms) upon connection instead of relying on default Windows BLE negotiation (30 ms – 50 ms).
- **1 ms Kernel Timer Resolution**: Uses `timeBeginPeriod(1)` to reduce OS scheduling latency and thread jitter.
- **MMCSS Pro Audio Scheduling**: Assigns threads to the Windows Multimedia Class Scheduler Service with `TIME_CRITICAL` priority.
- **Full MPE Support**: Preserves all 5 dimensions of touch across all 16 channels:
  - Note On/Off with velocity and release velocity
  - High-resolution 14-bit pitch bend
  - Continuous CC74 (Slide/Timbre)
  - Channel pressure and polyphonic aftertouch
- **Zero-Allocation Parser**: Stream-based BLE-MIDI 1.0 packet decoder handling timestamps, running status, SysEx fragmentation, and real-time clock de-interleaving.
- **Bidirectional Support**: Forwards outbound MIDI/SysEx from DAWs and configuration software (e.g., ROLI Dashboard) back to the controller.
- **Virtual MIDI Port**: Uses the `teVirtualMIDI` kernel driver for DAW integration.
- **Silent Background Process**: Built as a Windows GUI subsystem application (`windows_subsystem = "windows"`). No console window appears unless run with `--headless`.
- **System Tray Controls**: Quick access to connection status, device reconnection, and auto-start on boot.
- **Auto-Reconnect**: Re-arms scanning and automatically reconnects when the controller is powered on or returns to range.

## Requirements

- **Windows 10** (build 1809+) or **Windows 11**
- Bluetooth adapter supporting Bluetooth Low Energy (BLE 4.0+)
- **teVirtualMIDI driver** installed (comes bundled with [loopMIDI](https://www.tobias-erichsen.de/software/loopmidi.html) or [rtpMIDI](https://www.tobias-erichsen.de/software/rtpmidi.html))

## Installation

### Pre-built Binary
Download `bluemidi.exe` from the latest release and run it. The application will minimize to your system tray.

### Building from Source

Prerequisites: [Rust toolchain](https://rustup.rs/) (edition 2024 / Rust 1.85+).

```powershell
git clone https://github.com/username/bluemidi.git
cd bluemidi
cargo build --release
```

The optimized binary will be in `target/release/bluemidi.exe`.

## Usage

### Starting the Application

Double-click `bluemidi.exe` or execute:

```powershell
.\bluemidi.exe
```

The app starts silently and places an icon in your taskbar notification area.

### Command-Line Arguments

```
Usage: bluemidi.exe [OPTIONS]

Options:
  -p, --port <NAME>      Set virtual MIDI port name (default: "BlueMIDI - ROLI")
  -d, --device <FILTER>  Device name filter (default: matches ROLI/LUMI/Piano)
      --headless         Run as a console process without system tray icon
      --autostart        Enable running automatically on Windows startup
      --no-autostart     Disable running on Windows startup
  -h, --help             Show help screen
```

### Examples

Run with a custom port name:
```powershell
.\bluemidi.exe --port "My Instrument"
```

Run in console mode with event statistics:
```powershell
.\bluemidi.exe --headless
```

Filter by specific device name:
```powershell
.\bluemidi.exe --device "Seaboard"
```

## DAW Configuration

1. Open your DAW (Ableton Live, Bitwig Studio, Reaper, FL Studio, Cubase, etc.).
2. In MIDI Preferences, enable **BlueMIDI - ROLI** as a MIDI input.
3. Enable **MPE** on the MIDI input port:
   - **Ableton Live**: In Preferences > MIDI, expand the input port for BlueMIDI and check **MPE**.
   - **Bitwig Studio**: Add a new controller > select Generic MPE Keyboard > assign input to **BlueMIDI - ROLI**.
   - **ROLI Equator 2**: Select **BlueMIDI - ROLI** under MIDI settings; set MPE mode to **On**.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
# bluemidi
