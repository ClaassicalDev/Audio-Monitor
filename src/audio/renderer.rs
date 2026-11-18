use crate::audio::capture::CaptureWorker;
use crate::audio::dsp::{
    calculate_peak_db, soft_clip, FocusDucker, SidechainCompressor, SpectralDuckingFilter,
    VoiceClarityProcessor,
};
use crate::audio::process::{
    find_root_pid, get_all_processes, get_user_processes, resolve_root_pids, RunningProcess,
};
use crate::audio::ring_buffer::AudioRingBuffer;
use crate::config::AppConfig;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use wasapi::{
    initialize_mta, AudioClient, AudioRenderClient, DeviceEnumerator, Direction,
    SampleType, StreamMode, WaveFormat,
};

#[derive(Debug, Clone)]
pub struct AudioOutputDevice {
    pub id: String,
    pub name: String,
    #[allow(dead_code)]
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct ChannelLiveStatus {
    pub active: bool,
    pub pid: Option<u32>,
    pub vu_peak_db: f32,
}

impl Default for ChannelLiveStatus {
    fn default() -> Self {
        Self {
            active: false,
            pid: None,
            vu_peak_db: -100.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioEngineState {
    pub output_devices: Vec<AudioOutputDevice>,
    pub current_output_device_name: String,
    pub channel_status: HashMap<String, ChannelLiveStatus>,
    pub master_peak_db: f32,
    pub sidechain_reduction_db: f32,
    pub focus_reduction_db: f32,
    pub available_processes: Vec<RunningProcess>,
}

impl Default for AudioEngineState {
    fn default() -> Self {
        Self {
            output_devices: Vec::new(),
            current_output_device_name: "Por defecto (Windows)".into(),
            channel_status: HashMap::new(),
            master_peak_db: -100.0,
            sidechain_reduction_db: 0.0,
            focus_reduction_db: 0.0,
            available_processes: Vec::new(),
        }
    }
}

pub struct AudioEngine {
    pub state: Arc<RwLock<AudioEngineState>>,
    config: Arc<RwLock<AppConfig>>,
    stop_signal: Arc<AtomicBool>,
    mixer_thread: Option<JoinHandle<()>>,
    scanner_thread: Option<JoinHandle<()>>,
}

impl AudioEngine {
    pub fn new(config: AppConfig) -> Self {
        let state = Arc::new(RwLock::new(AudioEngineState::default()));
        let config = Arc::new(RwLock::new(config));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let state_mixer = state.clone();
        let config_mixer = config.clone();
        let stop_mixer = stop_signal.clone();

        let state_scanner = state.clone();
        let config_scanner = config.clone();
        let stop_scanner = stop_signal.clone();

        // Channels ring buffers map
        let ring_buffers: Arc<RwLock<HashMap<String, Arc<AudioRingBuffer>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let workers: Arc<RwLock<HashMap<String, CaptureWorker>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let ring_buffers_mixer = ring_buffers.clone();
        let ring_buffers_scanner = ring_buffers.clone();
        let workers_scanner = workers.clone();

        // 1. Process Scanner Thread (every 2 seconds)
        let scanner_thread = thread::Builder::new()
            .name("proc-scanner".into())
            .spawn(move || {
                let _ = initialize_mta();
                log::info!("AudioMonitor scanner thread started");

                while !stop_scanner.load(Ordering::Relaxed) {
                    let all_procs = get_all_processes();
                    let root_pids = resolve_root_pids(&all_procs);
                    let user_procs = get_user_processes(&all_procs);

                    // Update available processes
                    {
                        let mut st = state_scanner.write();
                        st.available_processes = user_procs;
                    }

                    // Refresh active devices
                    if let Ok(enumerator) = DeviceEnumerator::new() {
                        if let Ok(coll) = enumerator.get_device_collection(Direction::Render) {
                            if let Ok(count) = coll.get_nbr_devices() {
                                let mut devs = Vec::new();
                                devs.push(AudioOutputDevice {
                                    id: "default".into(),
                                    name: "Por defecto (Windows)".into(),
                                    is_default: true,
                                });
                                for i in 0..count {
                                    if let Ok(dev) = coll.get_device_at_index(i) {
                                        let id = dev.get_id().unwrap_or_default();
                                        let mut name = dev.get_friendlyname().unwrap_or_default();
                                        if name.to_lowercase().contains("cable") {
                                            name = format!("{} (CABLE - Virtual)", name);
                                        }
                                        devs.push(AudioOutputDevice {
                                            id,
                                            name,
                                            is_default: false,
                                        });
                                    }
                                }
                                let mut st = state_scanner.write();
                                st.output_devices = devs;
                            }
                        }
                    }

                    // Inspect and sync workers
                    let channels = { config_scanner.read().channels.clone() };
                    let mut active_channel_ids = Vec::new();

                    for ch in &channels {
                        active_channel_ids.push(ch.id.clone());

                        // Ensure ring buffer exists
                        let ring = {
                            let mut rings = ring_buffers_scanner.write();
                            rings
                                .entry(ch.id.clone())
                                .or_insert_with(|| AudioRingBuffer::new_shared(16384))
                                .clone()
                        };

                        if ch.exe_name == "__hardware_input__" {
                            let dev_match = ch.device_match.clone().unwrap_or_default();
                            let hw_connected = CaptureWorker::is_hardware_input_connected(&dev_match);
                            let mut w_lock = workers_scanner.write();
                            if hw_connected {
                                let need_start = match w_lock.get(&ch.id) {
                                    Some(w) => !w.is_running.load(Ordering::Relaxed),
                                    None => true,
                                };
                                if need_start {
                                    if let Some(mut old) = w_lock.remove(&ch.id) {
                                        old.stop();
                                    }
                                    ring.clear();
                                    let worker = CaptureWorker::start_hardware_input(
                                        ch.id.clone(),
                                        dev_match,
                                        ring,
                                    );
                                    w_lock.insert(ch.id.clone(), worker);
                                }
                                let mut st = state_scanner.write();
                                let status = st.channel_status.entry(ch.id.clone()).or_default();
                                status.active = true;
                                status.pid = None;
                            } else {
                                if let Some(mut old) = w_lock.remove(&ch.id) {
                                    old.stop();
                                }
                                ring.clear();
                                let mut st = state_scanner.write();
                                let status = st.channel_status.entry(ch.id.clone()).or_default();
                                status.active = false;
                                status.pid = None;
                                status.vu_peak_db = -100.0;
                            }
                        } else {
                            // Process capture
                            let found_pid = find_root_pid(&ch.exe_name, &root_pids);

                            let mut w_lock = workers_scanner.write();
                            match found_pid {
                                Some(pid) => {
                                    let need_restart = match w_lock.get(&ch.id) {
                                        Some(w) => {
                                            w.target_pid != Some(pid)
                                                || !w.is_running.load(Ordering::Relaxed)
                                        }
                                        None => true,
                                    };

                                    if need_restart {
                                        if let Some(mut old) = w_lock.remove(&ch.id) {
                                            old.stop();
                                        }
                                        ring.clear();
                                        let worker = CaptureWorker::start_process_loopback(
                                            ch.id.clone(),
                                            pid,
                                            ring,
                                        );
                                        w_lock.insert(ch.id.clone(), worker);
                                    }

                                    let mut st = state_scanner.write();
                                    let status = st.channel_status.entry(ch.id.clone()).or_default();
                                    status.active = true;
                                    status.pid = Some(pid);
                                }
                                None => {
                                    if let Some(mut old) = w_lock.remove(&ch.id) {
                                        old.stop();
                                    }
                                    let mut st = state_scanner.write();
                                    let status = st.channel_status.entry(ch.id.clone()).or_default();
                                    status.active = false;
                                    status.pid = None;
                                }
                            }
                        }
                    }

                    // Clean up removed channels
                    {
                        let mut w_lock = workers_scanner.write();
                        w_lock.retain(|k, v| {
                            if !active_channel_ids.contains(k) {
                                v.stop();
                                false
                            } else {
                                true
                            }
                        });
                    }

                    // Sleep 1.5 seconds between scans
                    for _ in 0..15 {
                        if stop_scanner.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }

                // Cleanup all workers on exit
                let mut w_lock = workers_scanner.write();
                for (_, mut w) in w_lock.drain() {
                    w.stop();
                }
            })
            .ok();

        // 2. Audio Output & Mixer Thread
        let mixer_thread = thread::Builder::new()
            .name("audio-mixer".into())
            .spawn(move || {
                let _ = initialize_mta();
                log::info!("AudioMonitor mixer thread started");

                let mut compressor = SidechainCompressor::new(48000.0);
                let mut focus_ducker = FocusDucker::new(48000.0);
                let mut voice_processors: HashMap<String, VoiceClarityProcessor> = HashMap::new();
                let mut spectral_filters: HashMap<String, SpectralDuckingFilter> = HashMap::new();
                let mut current_device_id = String::new();
                let mut audio_client_opt: Option<AudioClient> = None;
                let mut render_client_opt: Option<AudioRenderClient> = None;

                let open_device = |target_id: &str| -> Result<(AudioClient, AudioRenderClient, String), Box<dyn std::error::Error>> {
                    let enumerator = DeviceEnumerator::new()?;
                    let (device, dev_name) = if target_id == "default" || target_id.is_empty() {
                        let dev = enumerator.get_default_device(Direction::Render)?;
                        let name = dev.get_friendlyname().unwrap_or_else(|_| "Por defecto".into());
                        (dev, format!("Por defecto ({})", name))
                    } else {
                        let coll = enumerator.get_device_collection(Direction::Render)?;
                        let count = coll.get_nbr_devices()?;
                        let mut found = None;
                        for i in 0..count {
                            if let Ok(d) = coll.get_device_at_index(i) {
                                if let Ok(id) = d.get_id() {
                                    if id == target_id {
                                        let name = d.get_friendlyname().unwrap_or_else(|_| "Audio Device".into());
                                        found = Some((d, name));
                                        break;
                                    }
                                }
                            }
                        }
                        match found {
                            Some(pair) => pair,
                            None => {
                                let dev = enumerator.get_default_device(Direction::Render)?;
                                let name = dev.get_friendlyname().unwrap_or_else(|_| "Por defecto".into());
                                (dev, format!("Por defecto ({})", name))
                            }
                        }
                    };

                    let mut client = device.get_iaudioclient()?;
                    let format = WaveFormat::new(32, 32, SampleType::Float, 48000, 2, None);
                    let mode = StreamMode::PollingShared {
                        autoconvert: true,
                        buffer_duration_hns: 15_0000, // 15 ms buffer
                    };

                    client.initialize_client(&format, Direction::Render, mode, None)?;
                    let render_client = client.get_audiorenderclient()?;
                    client.start_stream()?;

                    Ok((client, render_client, dev_name))
                };

                let mut last_dev_check = Instant::now();
                let mut active_dev_id_str = String::new();

                while !stop_mixer.load(Ordering::Relaxed) {
                    let req_device_id = { config_mixer.read().output_device_id.clone() };

                    // Periodically check if default device changed
                    if last_dev_check.elapsed() > Duration::from_millis(1000) {
                        last_dev_check = Instant::now();
                        if req_device_id == "default" {
                            if let Ok(enumerator) = DeviceEnumerator::new() {
                                if let Ok(dev) = enumerator.get_default_device(Direction::Render) {
                                    if let Ok(id) = dev.get_id() {
                                        if id != active_dev_id_str && !active_dev_id_str.is_empty() {
                                            log::info!("Default audio device changed to {}", id);
                                            audio_client_opt = None;
                                            render_client_opt = None;
                                        }
                                        active_dev_id_str = id;
                                    }
                                }
                            }
                        }
                    }

                    // Open/Re-open audio client if needed
                    if audio_client_opt.is_none() || current_device_id != req_device_id {
                        match open_device(&req_device_id) {
                            Ok((client, render, dev_name)) => {
                                audio_client_opt = Some(client);
                                render_client_opt = Some(render);
                                current_device_id = req_device_id.clone();
                                let mut st = state_mixer.write();
                                st.current_output_device_name = dev_name;
                            }
                            Err(e) => {
                                log::debug!("Failed to open output device {}: {:?}", req_device_id, e);
                                thread::sleep(Duration::from_millis(50));
                                continue;
                            }
                        }
                    }

                    let (client, render_client) = match (audio_client_opt.as_mut(), render_client_opt.as_ref()) {
                        (Some(c), Some(r)) => (c, r),
                        _ => {
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    };

                    let space = match client.get_available_space_in_frames() {
                        Ok(s) => s as usize,
                        Err(e) => {
                            log::debug!("Error getting available space: {:?}", e);
                            audio_client_opt = None;
                            render_client_opt = None;
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    };

                    if space == 0 {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }

                    let frames_to_mix = space.min(2048);

                    // Fetch config snapshot
                    let cfg_snap = { config_mixer.read().clone() };

                    // Read channel buffers
                    let mut channel_frames: HashMap<String, Vec<[f32; 2]>> = HashMap::new();
                    let mut trigger_buf = vec![[0.0f32, 0.0f32]; frames_to_mix];
                    let mut prio2_buf = vec![[0.0f32, 0.0f32]; frames_to_mix];

                    {
                        let rings = ring_buffers_mixer.read();
                        for ch in &cfg_snap.channels {
                            if let Some(ring) = rings.get(&ch.id) {
                                let mut buf = vec![[0.0f32, 0.0f32]; frames_to_mix];
                                let popped = ring.pop_frames(&mut buf);
                                // Pad remainder with 0.0 if any
                                for i in popped..frames_to_mix {
                                    buf[i] = [0.0, 0.0];
                                }

                                if ch.is_trigger && !ch.muted {
                                    for i in 0..frames_to_mix {
                                        trigger_buf[i][0] += buf[i][0];
                                        trigger_buf[i][1] += buf[i][1];
                                    }
                                }

                                if ch.is_priority2 && !ch.muted {
                                    for i in 0..frames_to_mix {
                                        prio2_buf[i][0] += buf[i][0];
                                        prio2_buf[i][1] += buf[i][1];
                                    }
                                }

                                channel_frames.insert(ch.id.clone(), buf);
                            }
                        }
                    }

                    let mut master_frames = vec![[0.0f32, 0.0f32]; frames_to_mix];
                    let mut channel_peaks: HashMap<String, f32> = HashMap::new();

                    for frame_idx in 0..frames_to_mix {
                        let trig_l = trigger_buf[frame_idx][0];
                        let trig_r = trigger_buf[frame_idx][1];
                        let _sidechain_linear = compressor.process_frame(
                            trig_l,
                            trig_r,
                            &cfg_snap.compressor,
                        );

                        let prio2_l = prio2_buf[frame_idx][0];
                        let prio2_r = prio2_buf[frame_idx][1];
                        let _focus_reduction = if cfg_snap.focus_mode_enabled {
                            focus_ducker.process_frame(
                                prio2_l,
                                prio2_r,
                                cfg_snap.focus_threshold_db,
                                cfg_snap.focus_duck_db,
                            )
                        } else {
                            focus_ducker.reset();
                            0.0
                        };

                        let mut sample_l = 0.0f32;
                        let mut sample_r = 0.0f32;

                        for ch in &cfg_snap.channels {
                            if let Some(buf) = channel_frames.get(&ch.id) {
                                let raw_frame = buf[frame_idx];
                                if ch.muted {
                                    continue;
                                }

                                // Apply Studio Voice Clarity DSP if enabled globally and on this channel
                                let frame_to_mix = if cfg_snap.voice_clarity_enabled && ch.voice_clarity {
                                    let proc = voice_processors
                                        .entry(ch.id.clone())
                                        .or_insert_with(|| VoiceClarityProcessor::new(48000.0, cfg_snap.voice_clarity_mode));
                                    proc.process_frame(
                                        raw_frame[0],
                                        raw_frame[1],
                                        cfg_snap.voice_clarity_mode,
                                        cfg_snap.deesser_enabled,
                                        cfg_snap.noise_gate_enabled,
                                    )
                                } else {
                                    raw_frame
                                };

                                let mut gain = ch.volume;
                                if !ch.exempt_master {
                                    gain *= cfg_snap.master_volume;
                                }

                                // Determine ducking amount based on 3-tier hierarchy:
                                // Level 1 (Discord trigger): No ducking
                                // Level 2 (Priority 2): Only Discord ducking (if use_compressor)
                                // Level 3 (Normal): Combined Discord + Focus Mode ducking (if use_compressor)
                                let (l, r) = if ch.is_trigger {
                                    (frame_to_mix[0] * gain, frame_to_mix[1] * gain)
                                } else if ch.is_priority2 {
                                    if ch.use_compressor {
                                        let flt = spectral_filters
                                            .entry(ch.id.clone())
                                            .or_insert_with(|| SpectralDuckingFilter::new(48000.0));
                                        let ducked = flt.process_frame(
                                            frame_to_mix[0],
                                            frame_to_mix[1],
                                            compressor.current_reduction_db,
                                            cfg_snap.compressor.spectral_ducking,
                                            cfg_snap.compressor.spectral_fullband_ratio,
                                        );
                                        (ducked[0] * gain, ducked[1] * gain)
                                    } else {
                                        (frame_to_mix[0] * gain, frame_to_mix[1] * gain)
                                    }
                                } else if ch.use_compressor {
                                    let total_reduction = (compressor.current_reduction_db + focus_ducker.current_reduction_db).max(-45.0);
                                    let flt = spectral_filters
                                        .entry(ch.id.clone())
                                        .or_insert_with(|| SpectralDuckingFilter::new(48000.0));
                                    let ducked = flt.process_frame(
                                        frame_to_mix[0],
                                        frame_to_mix[1],
                                        total_reduction,
                                        cfg_snap.compressor.spectral_ducking,
                                        cfg_snap.compressor.spectral_fullband_ratio,
                                    );
                                    (ducked[0] * gain, ducked[1] * gain)
                                } else {
                                    (frame_to_mix[0] * gain, frame_to_mix[1] * gain)
                                };

                                sample_l += l;
                                sample_r += r;

                                let peak = l.abs().max(r.abs());
                                let entry = channel_peaks.entry(ch.id.clone()).or_insert(0.0);
                                if peak > *entry {
                                    *entry = peak;
                                }
                            }
                        }

                        master_frames[frame_idx] = [soft_clip(sample_l), soft_clip(sample_r)];
                    }

                    // Convert to raw bytes and write to WASAPI render client
                    let byte_slice: &[u8] = bytemuck::cast_slice(&master_frames);
                    if let Err(e) = render_client.write_to_device(byte_slice, None) {
                        log::debug!("Error writing to render device: {:?}", e);
                        audio_client_opt = None;
                        render_client_opt = None;
                        continue;
                    }

                    // Update live VU meters with smooth decay
                    let master_peak = calculate_peak_db(&master_frames);
                    let sidechain_reduction = compressor.current_reduction_db;
                    let focus_reduction = focus_ducker.current_reduction_db;

                    {
                        let mut st = state_mixer.write();
                        st.master_peak_db = master_peak;
                        st.sidechain_reduction_db = sidechain_reduction;
                        st.focus_reduction_db = focus_reduction;

                        for ch in &cfg_snap.channels {
                            let status = st.channel_status.entry(ch.id.clone()).or_default();
                            if ch.muted || !status.active {
                                status.vu_peak_db = -100.0;
                                continue;
                            }

                            let peak_linear = channel_peaks.get(&ch.id).copied().unwrap_or(0.0);
                            let peak_db = if peak_linear < 1e-5 {
                                -100.0
                            } else {
                                20.0 * peak_linear.log10()
                            };

                            // Exponential decay
                            if peak_db > status.vu_peak_db {
                                status.vu_peak_db = peak_db;
                            } else {
                                status.vu_peak_db = (status.vu_peak_db * 0.85 + peak_db * 0.15).max(-100.0);
                            }
                        }
                    }

                    thread::sleep(Duration::from_millis(3));
                }

                if let Some(client) = audio_client_opt {
                    let _ = client.stop_stream();
                }
            })
            .ok();

        Self {
            state,
            config,
            stop_signal,
            mixer_thread,
            scanner_thread,
        }
    }

    pub fn update_config(&self, new_config: AppConfig) {
        *self.config.write() = new_config;
    }

    pub fn stop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(handle) = self.mixer_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.scanner_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
