use crate::audio::ring_buffer::AudioRingBuffer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use wasapi::{
    initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat,
};

pub struct CaptureWorker {
    #[allow(dead_code)]
    pub channel_id: String,
    pub target_pid: Option<u32>,
    pub is_running: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    pub ring_buffer: Arc<AudioRingBuffer>,
}

impl CaptureWorker {
    /// Spawns a WASAPI process loopback capture worker for a specific process ID.
    pub fn start_process_loopback(
        channel_id: String,
        pid: u32,
        ring_buffer: Arc<AudioRingBuffer>,
    ) -> Self {
        let is_running = Arc::new(AtomicBool::new(true));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let is_running_clone = is_running.clone();
        let stop_signal_clone = stop_signal.clone();
        let ring_buffer_clone = ring_buffer.clone();

        let handle = thread::Builder::new()
            .name(format!("cap-{}", channel_id))
            .spawn(move || {
                let _ = initialize_mta();
                log::info!("Starting loopback capture for PID {}", pid);

                let run = || -> Result<(), Box<dyn std::error::Error>> {
                    let mut audio_client = AudioClient::new_application_loopback_client(pid, true)?;
                    let desired_format =
                        WaveFormat::new(32, 32, SampleType::Float, 48000, 2, None);
                    let mode = StreamMode::PollingShared {
                        autoconvert: true,
                        buffer_duration_hns: 20_0000, // 20 ms
                    };

                    audio_client.initialize_client(
                        &desired_format,
                        Direction::Capture,
                        mode,
                        None,
                    )?;

                    let capture_client = audio_client.get_audiocaptureclient()?;
                    audio_client.start_stream()?;

                    let mut raw_bytes = vec![0u8; 4096 * 8]; // up to 4096 stereo f32 frames

                    while !stop_signal_clone.load(Ordering::Relaxed) {
                        match capture_client.read_from_device(&mut raw_bytes) {
                            Ok((frames, buffer_info)) => {
                                if frames > 0 {
                                    let byte_count = frames as usize * 8;
                                    if !buffer_info.flags.silent {
                                        let slice = &raw_bytes[..byte_count];
                                        let samples: &[f32] = bytemuck::cast_slice(slice);
                                        let mut stereo_frames = Vec::with_capacity(frames as usize);
                                        for chunk in samples.chunks_exact(2) {
                                            stereo_frames.push([chunk[0], chunk[1]]);
                                        }
                                        ring_buffer_clone.push_frames(&stereo_frames);
                                    } else {
                                        let zeros = vec![[0.0f32, 0.0f32]; frames as usize];
                                        ring_buffer_clone.push_frames(&zeros);
                                    }
                                }
                            }
                            Err(e) => {
                                log::debug!("Capture read ended for PID {}: {:?}", pid, e);
                                break;
                            }
                        }
                        thread::sleep(Duration::from_millis(3));
                    }

                    let _ = audio_client.stop_stream();
                    Ok(())
                };

                if let Err(e) = run() {
                    log::debug!("Capture loop terminated for PID {}: {:?}", pid, e);
                }

                is_running_clone.store(false, Ordering::Relaxed);
            })
            .ok();

        Self {
            channel_id,
            target_pid: Some(pid),
            is_running,
            stop_signal,
            thread_handle: handle,
            ring_buffer,
        }
    }

    /// Checks if a matching hardware capture device (e.g. DigiTech RP500) is currently active/connected.
    pub fn is_hardware_input_connected(device_match: &str) -> bool {
        let Ok(enumerator) = DeviceEnumerator::new() else {
            return false;
        };
        let Ok(collection) = enumerator.get_device_collection(Direction::Capture) else {
            return false;
        };
        let Ok(count) = collection.get_nbr_devices() else {
            return false;
        };
        let target = device_match.to_lowercase();
        for i in 0..count {
            if let Ok(dev) = collection.get_device_at_index(i) {
                let id = dev.get_id().unwrap_or_default().to_lowercase();
                let name = dev.get_friendlyname().unwrap_or_default().to_lowercase();
                if (!target.is_empty() && (id.contains(&target) || name.contains(&target)))
                    || name.contains("digitech")
                    || name.contains("rp500")
                {
                    return true;
                }
            }
        }
        false
    }

    /// Spawns a Hardware input capture worker (e.g. for DigiTech RP500).
    pub fn start_hardware_input(
        channel_id: String,
        device_match: String,
        ring_buffer: Arc<AudioRingBuffer>,
    ) -> Self {
        let is_running = Arc::new(AtomicBool::new(true));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let is_running_clone = is_running.clone();
        let stop_signal_clone = stop_signal.clone();
        let ring_buffer_clone = ring_buffer.clone();

        let handle = thread::Builder::new()
            .name(format!("cap-hw-{}", channel_id))
            .spawn(move || {
                let _ = initialize_mta();
                log::info!("Starting hardware input capture for match: {}", device_match);

                let run = || -> Result<(), Box<dyn std::error::Error>> {
                    let enumerator = DeviceEnumerator::new()?;
                    let collection = enumerator.get_device_collection(Direction::Capture)?;
                    let count = collection.get_nbr_devices()?;

                    let mut matched_device = None;
                    for i in 0..count {
                        if let Ok(dev) = collection.get_device_at_index(i) {
                            let id = dev.get_id().unwrap_or_default().to_lowercase();
                            let name = dev.get_friendlyname().unwrap_or_default().to_lowercase();
                            let target = device_match.to_lowercase();
                            if id.contains(&target)
                                || name.contains(&target)
                                || name.contains("digitech")
                                || name.contains("rp500")
                            {
                                matched_device = Some(dev);
                                break;
                            }
                        }
                    }

                    let device = match matched_device {
                        Some(d) => d,
                        None => {
                            log::info!("Hardware device {} not found", device_match);
                            return Ok(());
                        }
                    };

                    let mut audio_client = device.get_iaudioclient()?;
                    let desired_format =
                        WaveFormat::new(32, 32, SampleType::Float, 48000, 2, None);
                    let mode = StreamMode::PollingShared {
                        autoconvert: true,
                        buffer_duration_hns: 20_0000,
                    };

                    audio_client.initialize_client(
                        &desired_format,
                        Direction::Capture,
                        mode,
                        None,
                    )?;

                    let capture_client = audio_client.get_audiocaptureclient()?;
                    audio_client.start_stream()?;

                    let mut raw_bytes = vec![0u8; 4096 * 8];

                    while !stop_signal_clone.load(Ordering::Relaxed) {
                        match capture_client.read_from_device(&mut raw_bytes) {
                            Ok((frames, buffer_info)) => {
                                if frames > 0 {
                                    let byte_count = frames as usize * 8;
                                    if !buffer_info.flags.silent {
                                        let slice = &raw_bytes[..byte_count];
                                        let samples: &[f32] = bytemuck::cast_slice(slice);
                                        let mut stereo_frames = Vec::with_capacity(frames as usize);
                                        for chunk in samples.chunks_exact(2) {
                                            stereo_frames.push([chunk[0], chunk[1]]);
                                        }
                                        ring_buffer_clone.push_frames(&stereo_frames);
                                    } else {
                                        let zeros = vec![[0.0f32, 0.0f32]; frames as usize];
                                        ring_buffer_clone.push_frames(&zeros);
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                        thread::sleep(Duration::from_millis(3));
                    }

                    let _ = audio_client.stop_stream();
                    Ok(())
                };

                if let Err(e) = run() {
                    log::debug!("Hardware capture error: {:?}", e);
                }

                is_running_clone.store(false, Ordering::Relaxed);
            })
            .ok();

        Self {
            channel_id,
            target_pid: None,
            is_running,
            stop_signal,
            thread_handle: handle,
            ring_buffer,
        }
    }

    pub fn stop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        self.is_running.store(false, Ordering::Relaxed);
    }
}

impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
