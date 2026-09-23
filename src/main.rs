#![windows_subsystem = "windows"]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use crossbeam_channel::unbounded;

use bluemidi::ble::{BleMidiManager, BleStatus};
use bluemidi::midi::{BleMidiEncoder, BleMidiParser, VirtualMidiPort};
use bluemidi::system::autostart::set_autostart;
use bluemidi::system::mmcss::MmcssGuard;
use bluemidi::system::tray::TrayManager;

#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

struct HighResolutionTimerGuard;

impl HighResolutionTimerGuard {
    fn new() -> Self {
        unsafe {
            let _ = timeBeginPeriod(1);
        }
        Self
    }
}

impl Drop for HighResolutionTimerGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = timeEndPeriod(1);
        }
    }
}

#[derive(Default)]
struct MidiStats {
    total_messages: AtomicU64,
    notes: AtomicU64,
    pitch_bends: AtomicU64,
    pressure: AtomicU64,
    slide_cc74: AtomicU64,
}

fn print_help() {
    println!(r#"BlueMIDI - Bluetooth Low Energy to MIDI Bridge for Windows

Usage:
  bluemidi.exe [OPTIONS]

Options:
  -p, --port <NAME>      Set virtual MIDI port name (default: "BlueMIDI - ROLI")
  -d, --device <FILTER>  Bluetooth device name filter (default: matches ROLI/LUMI/Piano)
      --headless         Run as a console process without system tray icon
      --autostart        Enable running automatically on Windows startup
      --no-autostart     Disable running on Windows startup
  -h, --help             Show this help screen
"#);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut port_name = "BlueMIDI - ROLI".to_string();
    let mut device_filter = String::new();
    let mut headless = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                attach_parent_console();
                print_help();
                return;
            }
            "-p" | "--port" => {
                if i + 1 < args.len() {
                    port_name = args[i + 1].clone();
                    i += 1;
                }
            }
            "-d" | "--device" => {
                if i + 1 < args.len() {
                    device_filter = args[i + 1].clone();
                    i += 1;
                }
            }
            "--headless" => {
                headless = true;
                attach_parent_console();
            }
            "--autostart" => {
                attach_parent_console();
                match set_autostart(true) {
                    Ok(()) => println!("Auto-start enabled."),
                    Err(e) => eprintln!("Failed to enable auto-start: {}", e),
                }
                return;
            }
            "--no-autostart" => {
                attach_parent_console();
                match set_autostart(false) {
                    Ok(()) => println!("Auto-start disabled."),
                    Err(e) => eprintln!("Failed to disable auto-start: {}", e),
                }
                return;
            }
            _ => {}
        }
        i += 1;
    }

    if headless {
        println!("BlueMIDI active. Port: {}, Filter: {}", port_name, if device_filter.is_empty() { "Auto" } else { &device_filter });
    }

    let _mmcss = MmcssGuard::new("Pro Audio");
    let _timer_guard = HighResolutionTimerGuard::new();

    let (packet_tx, packet_rx) = unbounded::<Vec<u8>>();
    let (outbound_tx, outbound_rx) = unbounded::<Vec<u8>>();
    let (status_tx, status_rx) = unbounded::<BleStatus>();

    let should_stop = Arc::new(AtomicBool::new(false));
    let reconnect_trigger = Arc::new(AtomicBool::new(false));

    let should_stop_ctrlc = Arc::clone(&should_stop);
    ctrlc_handler(move || {
        should_stop_ctrlc.store(true, Ordering::SeqCst);
    });

    let outbound_tx_clone = outbound_tx.clone();
    let encoder = Arc::new(BleMidiEncoder::new(64));
    let encoder_clone = Arc::clone(&encoder);

    let virtual_port = match VirtualMidiPort::new(
        &port_name,
        Some(Box::new(move |midi_data: &[u8]| {
            let ble_packets = encoder_clone.encode(midi_data);
            for p in ble_packets {
                let _ = outbound_tx_clone.send(p);
            }
        })),
    ) {
        Ok(port) => Arc::new(port),
        Err(e) => {
            attach_parent_console();
            eprintln!("Error initializing virtual MIDI port: {}", e);
            return;
        }
    };

    let virtual_port_clone = Arc::clone(&virtual_port);
    let should_stop_worker = Arc::clone(&should_stop);
    let stats = Arc::new(MidiStats::default());
    let stats_clone = Arc::clone(&stats);

    std::thread::spawn(move || {
        let _worker_mmcss = MmcssGuard::new("Pro Audio");
        let mut parser = BleMidiParser::new();

        while !should_stop_worker.load(Ordering::Relaxed) {
            match packet_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(raw_packet) => {
                    parser.parse(&raw_packet, |msg| {
                        virtual_port_clone.send(msg);

                        stats_clone.total_messages.fetch_add(1, Ordering::Relaxed);
                        if !msg.is_empty() {
                            match msg[0] & 0xF0 {
                                0x90 if msg.len() > 2 && msg[2] > 0 => {
                                    stats_clone.notes.fetch_add(1, Ordering::Relaxed);
                                }
                                0xE0 => {
                                    stats_clone.pitch_bends.fetch_add(1, Ordering::Relaxed);
                                }
                                0xB0 if msg.len() > 2 && msg[1] == 74 => {
                                    stats_clone.slide_cc74.fetch_add(1, Ordering::Relaxed);
                                }
                                0xD0 | 0xA0 => {
                                    stats_clone.pressure.fetch_add(1, Ordering::Relaxed);
                                }
                                _ => {}
                            }
                        }
                    });
                }
                Err(_) => {}
            }
        }
    });

    let ble_manager = BleMidiManager::new(
        device_filter,
        status_tx,
        packet_tx,
        outbound_rx,
    );
    let should_stop_ble = Arc::clone(&should_stop);
    let reconnect_trigger_ble = Arc::clone(&reconnect_trigger);

    std::thread::spawn(move || {
        let _ble_mmcss = MmcssGuard::new("Pro Audio");
        ble_manager.run(should_stop_ble, reconnect_trigger_ble);
    });

    if !headless {
        let tray = match TrayManager::new("Scanning for device...", &port_name) {
            Ok(t) => Some(t),
            Err(_) => None,
        };

        while !should_stop.load(Ordering::Relaxed) {
            while let Ok(status) = status_rx.try_recv() {
                match status {
                    BleStatus::Scanning => {
                        if let Some(ref t) = tray {
                            t.update_status("Scanning for device...");
                        }
                    }
                    BleStatus::Connecting(name) => {
                        if let Some(ref t) = tray {
                            t.update_status(&format!("Connecting to {}...", name));
                        }
                    }
                    BleStatus::Connected { name, latency_optimized, .. } => {
                        let latency_text = if latency_optimized { "7.5ms" } else { "normal" };
                        if let Some(ref t) = tray {
                            t.update_status(&format!("Connected: {} ({})", name, latency_text));
                        }
                    }
                    BleStatus::Disconnected => {
                        if let Some(ref t) = tray {
                            t.update_status("Disconnected. Reconnecting...");
                        }
                    }
                    BleStatus::Error(err) => {
                        if let Some(ref t) = tray {
                            t.update_status(&format!("Error: {}", err));
                        }
                    }
                    BleStatus::Idle => {}
                }
            }

            if let Some(ref t) = tray {
                if t.should_reconnect() {
                    reconnect_trigger.store(true, Ordering::SeqCst);
                }

                if !t.pump_messages() {
                    should_stop.store(true, Ordering::SeqCst);
                    break;
                }
            }

            std::thread::sleep(Duration::from_millis(16));
        }
    } else {
        println!("Running in console mode. Press Ctrl+C to exit.");
        let mut last_stats_print = std::time::Instant::now();

        while !should_stop.load(Ordering::Relaxed) {
            while let Ok(status) = status_rx.try_recv() {
                match status {
                    BleStatus::Scanning => println!("Scanning for device..."),
                    BleStatus::Connecting(name) => println!("Connecting to '{}'...", name),
                    BleStatus::Connected { name, latency_optimized, .. } => {
                        let latency_text = if latency_optimized { "7.5ms" } else { "normal" };
                        println!("Connected: '{}' ({})", name, latency_text);
                    }
                    BleStatus::Disconnected => println!("Disconnected. Reconnecting..."),
                    BleStatus::Error(err) => eprintln!("Error: {}", err),
                    BleStatus::Idle => {}
                }
            }

            if last_stats_print.elapsed() >= Duration::from_secs(10) {
                let total = stats.total_messages.load(Ordering::Relaxed);
                if total > 0 {
                    println!(
                        "Messages: {} | Notes: {} | Pitch Bend: {} | CC74: {} | Pressure: {}",
                        total,
                        stats.notes.load(Ordering::Relaxed),
                        stats.pitch_bends.load(Ordering::Relaxed),
                        stats.slide_cc74.load(Ordering::Relaxed),
                        stats.pressure.load(Ordering::Relaxed),
                    );
                }
                last_stats_print = std::time::Instant::now();
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn attach_parent_console() {
    unsafe {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn ctrlc_handler<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    use std::sync::Mutex;
    let callback = Arc::new(Mutex::new(Some(f)));

    unsafe {
        use windows::Win32::System::Console::SetConsoleCtrlHandler;
        static mut HANDLER_CB: Option<Box<dyn Fn() + Send + 'static>> = None;

        let cb_clone = Arc::clone(&callback);
        HANDLER_CB = Some(Box::new(move || {
            if let Ok(mut lock) = cb_clone.lock() {
                if let Some(cb) = lock.take() {
                    cb();
                }
            }
        }));

        unsafe extern "system" fn native_ctrl_handler(_ctrl_type: u32) -> windows::core::BOOL {
            unsafe {
                if let Some(ref cb) = HANDLER_CB {
                    cb();
                }
            }
            windows::core::BOOL(1)
        }

        let _ = SetConsoleCtrlHandler(Some(native_ctrl_handler), true);
    }
}
