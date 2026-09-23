use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use crossbeam_channel::{Receiver, Sender};
use windows::core::GUID;
use windows::Devices::Bluetooth::Advertisement::*;
use windows::Devices::Bluetooth::GenericAttributeProfile::*;
use windows::Devices::Bluetooth::*;
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter};

pub const BLE_MIDI_SERVICE_UUID: GUID = GUID::from_u128(0x03b80e5a_ede8_4b33_a751_6ce34ec4c700);
pub const BLE_MIDI_CHAR_UUID: GUID = GUID::from_u128(0x7772e5db_3868_4112_a1a9_f2669d106bf3);

#[derive(Clone, Debug)]
pub struct DiscoveredDevice {
    pub name: String,
    pub address: u64,
    pub rssi: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BleStatus {
    Idle,
    Scanning,
    Connecting(String),
    Connected {
        name: String,
        address: u64,
        latency_optimized: bool,
    },
    Disconnected,
    Error(String),
}

pub struct BleMidiSession {
    pub device_name: String,
    pub address: u64,
    characteristic: GattCharacteristic,
    _device: BluetoothLEDevice,
    connected: Arc<AtomicBool>,
}

impl BleMidiSession {
    pub fn write_packet(&self, packet: &[u8]) -> Result<(), String> {
        if !self.connected.load(Ordering::Relaxed) || packet.is_empty() {
            return Ok(());
        }

        let writer = DataWriter::new().map_err(|e| e.to_string())?;
        writer.WriteBytes(packet).map_err(|e| e.to_string())?;
        let buffer = writer.DetachBuffer().map_err(|e| e.to_string())?;

        let op = self
            .characteristic
            .WriteValueWithOptionAsync(&buffer, GattWriteOption::WriteWithoutResponse)
            .map_err(|e| e.to_string())?;

        let _ = pollster::block_on(op);
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

pub struct BleMidiManager {
    device_filter: String,
    status_sender: Sender<BleStatus>,
    packet_sender: Sender<Vec<u8>>,
    outbound_receiver: Receiver<Vec<u8>>,
}

impl BleMidiManager {
    pub fn new(
        device_filter: String,
        status_sender: Sender<BleStatus>,
        packet_sender: Sender<Vec<u8>>,
        outbound_receiver: Receiver<Vec<u8>>,
    ) -> Self {
        Self {
            device_filter,
            status_sender,
            packet_sender,
            outbound_receiver,
        }
    }

    pub fn run(&self, should_stop: Arc<AtomicBool>, reconnect_trigger: Arc<AtomicBool>) {
        while !should_stop.load(Ordering::Relaxed) {
            let _ = self.status_sender.send(BleStatus::Scanning);

            let target = match self.scan_for_device(&should_stop, &reconnect_trigger) {
                Ok(Some(dev)) => dev,
                Ok(None) => {
                    if should_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                Err(e) => {
                    let _ = self.status_sender.send(BleStatus::Error(e));
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let _ = self.status_sender.send(BleStatus::Connecting(target.name.clone()));

            match self.connect_device(&target, &should_stop, &reconnect_trigger) {
                Ok(()) => {
                    let _ = self.status_sender.send(BleStatus::Disconnected);
                }
                Err(e) => {
                    let _ = self.status_sender.send(BleStatus::Error(format!("Connection error: {}", e)));
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }

        let _ = self.status_sender.send(BleStatus::Idle);
    }

    fn scan_for_device(
        &self,
        should_stop: &Arc<AtomicBool>,
        reconnect_trigger: &Arc<AtomicBool>,
    ) -> Result<Option<DiscoveredDevice>, String> {
        let watcher = BluetoothLEAdvertisementWatcher::new().map_err(|e| e.to_string())?;
        watcher
            .SetScanningMode(BluetoothLEScanningMode::Active)
            .map_err(|e| e.to_string())?;

        let discovered = Arc::new(std::sync::Mutex::new(None));
        let discovered_clone = Arc::clone(&discovered);
        let filter = self.device_filter.to_lowercase();

        watcher
            .Received(&TypedEventHandler::new(
                move |_sender: windows::core::Ref<'_, BluetoothLEAdvertisementWatcher>,
                      args: windows::core::Ref<'_, BluetoothLEAdvertisementReceivedEventArgs>| {
                    if let Some(args) = args.as_ref() {
                        let adv = args.Advertisement()?;
                        let name = adv.LocalName()?.to_string();
                        let addr = args.BluetoothAddress()?;
                        let rssi = args.RawSignalStrengthInDBm()?;

                        let mut is_match = false;

                        if !filter.is_empty() {
                            if name.to_lowercase().contains(&filter) {
                                is_match = true;
                            }
                        } else {
                            let name_lower = name.to_lowercase();
                            if name_lower.contains("roli")
                                || name_lower.contains("lumi")
                                || name_lower.contains("seaboard")
                                || name_lower.contains("piano")
                                || name_lower.contains("block")
                            {
                                is_match = true;
                            }
                        }

                        if !is_match {
                            if let Ok(uuids) = adv.ServiceUuids() {
                                for uuid in uuids {
                                    if uuid == BLE_MIDI_SERVICE_UUID {
                                        is_match = true;
                                        break;
                                    }
                                }
                            }
                        }

                        if is_match {
                            let mut lock = discovered_clone.lock().unwrap();
                            if lock.is_none() {
                                *lock = Some(DiscoveredDevice {
                                    name: if name.is_empty() { "BLE MIDI Device".to_string() } else { name },
                                    address: addr,
                                    rssi,
                                });
                            }
                        }
                    }
                    Ok(())
                },
            ))
            .map_err(|e| e.to_string())?;

        watcher.Start().map_err(|e| e.to_string())?;

        let mut found = None;
        while !should_stop.load(Ordering::Relaxed) && !reconnect_trigger.load(Ordering::Relaxed) {
            {
                let lock = discovered.lock().unwrap();
                if let Some(ref dev) = *lock {
                    found = Some(dev.clone());
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let _ = watcher.Stop();
        reconnect_trigger.store(false, Ordering::Relaxed);
        Ok(found)
    }

    fn connect_device(
        &self,
        target: &DiscoveredDevice,
        should_stop: &Arc<AtomicBool>,
        reconnect_trigger: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        let op = BluetoothLEDevice::FromBluetoothAddressAsync(target.address)
            .map_err(|e| format!("Failed to open BLE device request: {}", e))?;
        let device = pollster::block_on(op)
            .map_err(|e| format!("Failed to open BLE device: {}", e))?;

        let mut latency_optimized = false;
        if let Ok(params) = BluetoothLEPreferredConnectionParameters::ThroughputOptimized() {
            if device.RequestPreferredConnectionParameters(&params).is_ok() {
                latency_optimized = true;
            }
        }

        let service_op = device.GetGattServicesForUuidAsync(BLE_MIDI_SERVICE_UUID)
            .map_err(|e| e.to_string())?;
        let service_res = pollster::block_on(service_op)
            .map_err(|e| e.to_string())?;

        if service_res.Status().map_err(|e| e.to_string())? != GattCommunicationStatus::Success {
            return Err("BLE MIDI Service not found or device rejected connection".to_string());
        }

        let services = service_res.Services().map_err(|e| e.to_string())?;
        if services.Size().map_err(|e| e.to_string())? == 0 {
            return Err("BLE MIDI Service UUID list is empty".to_string());
        }
        let service = services.GetAt(0).map_err(|e| e.to_string())?;

        let char_op = service.GetCharacteristicsForUuidAsync(BLE_MIDI_CHAR_UUID)
            .map_err(|e| e.to_string())?;
        let char_res = pollster::block_on(char_op)
            .map_err(|e| e.to_string())?;

        if char_res.Status().map_err(|e| e.to_string())? != GattCommunicationStatus::Success {
            return Err("BLE MIDI Characteristic not found".to_string());
        }

        let chars = char_res.Characteristics().map_err(|e| e.to_string())?;
        if chars.Size().map_err(|e| e.to_string())? == 0 {
            return Err("BLE MIDI Characteristic list is empty".to_string());
        }
        let characteristic = chars.GetAt(0).map_err(|e| e.to_string())?;

        let cccd_op = characteristic.WriteClientCharacteristicConfigurationDescriptorAsync(
            GattClientCharacteristicConfigurationDescriptorValue::Notify,
        )
        .map_err(|e| e.to_string())?;
        let cccd_res = pollster::block_on(cccd_op)
            .map_err(|e| e.to_string())?;

        if cccd_res != GattCommunicationStatus::Success {
            return Err(format!("Failed to enable BLE notifications (Status: {:?})", cccd_res));
        }

        let is_connected = Arc::new(AtomicBool::new(true));

        let packet_tx = self.packet_sender.clone();
        characteristic
            .ValueChanged(&TypedEventHandler::new(
                move |_sender: windows::core::Ref<'_, GattCharacteristic>,
                      args: windows::core::Ref<'_, GattValueChangedEventArgs>| {
                    if let Some(args) = args.as_ref() {
                        if let Ok(value) = args.CharacteristicValue() {
                            if let Ok(reader) = DataReader::FromBuffer(&value) {
                                if let Ok(len) = reader.UnconsumedBufferLength() {
                                    let mut buf = vec![0u8; len as usize];
                                    if reader.ReadBytes(&mut buf).is_ok() {
                                        let _ = packet_tx.send(buf);
                                    }
                                }
                            }
                        }
                    }
                    Ok(())
                },
            ))
            .map_err(|e| e.to_string())?;

        let conn_status_clone = Arc::clone(&is_connected);
        let _ = device.ConnectionStatusChanged(&TypedEventHandler::new(
            move |dev: windows::core::Ref<'_, BluetoothLEDevice>, _args| {
                if let Some(dev) = dev.as_ref() {
                    if let Ok(status) = dev.ConnectionStatus() {
                        if status == BluetoothConnectionStatus::Disconnected {
                            conn_status_clone.store(false, Ordering::SeqCst);
                        }
                    }
                }
                Ok(())
            },
        ));

        let _ = self.status_sender.send(BleStatus::Connected {
            name: target.name.clone(),
            address: target.address,
            latency_optimized,
        });

        while !should_stop.load(Ordering::Relaxed)
            && !reconnect_trigger.load(Ordering::Relaxed)
            && is_connected.load(Ordering::Relaxed)
        {
            while let Ok(out_packet) = self.outbound_receiver.try_recv() {
                if let Ok(writer) = DataWriter::new() {
                    if writer.WriteBytes(&out_packet).is_ok() {
                        if let Ok(buffer) = writer.DetachBuffer() {
                            let _ = characteristic.WriteValueWithOptionAsync(
                                &buffer,
                                GattWriteOption::WriteWithoutResponse,
                            );
                        }
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(1));
        }

        reconnect_trigger.store(false, Ordering::Relaxed);
        Ok(())
    }
}
