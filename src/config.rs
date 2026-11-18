use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

fn default_hold_ms() -> f32 {
    85.0
}

fn default_max_reduction_db() -> f32 {
    -15.0
}

fn default_preset_name() -> String {
    "Gaming Pro".to_string()
}

fn default_spectral_fullband_ratio() -> f32 {
    0.75
}

fn default_focus_duck_db() -> f32 {
    -12.0
}

fn default_focus_threshold_db() -> f32 {
    -42.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub master_volume: f32,
    pub output_device_id: String,
    pub show_only_active: bool,
    pub compressor: CompressorConfig,
    #[serde(default = "default_true")]
    pub voice_clarity_enabled: bool,
    #[serde(default)]
    pub voice_clarity_mode: u8,
    #[serde(default = "default_true")]
    pub deesser_enabled: bool,
    #[serde(default = "default_true")]
    pub noise_gate_enabled: bool,
    #[serde(default = "default_true")]
    pub focus_mode_enabled: bool,
    #[serde(default = "default_focus_duck_db")]
    pub focus_duck_db: f32,
    #[serde(default = "default_focus_threshold_db")]
    pub focus_threshold_db: f32,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    pub channels: Vec<ChannelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressorConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub output_gain_db: f32,
    #[serde(default = "default_hold_ms")]
    pub hold_ms: f32,
    #[serde(default = "default_max_reduction_db")]
    pub max_reduction_db: f32,
    #[serde(default = "default_preset_name")]
    pub preset_name: String,
    #[serde(default = "default_true")]
    pub spectral_ducking: bool,
    #[serde(default = "default_true")]
    pub voice_sc_filter: bool,
    #[serde(default = "default_spectral_fullband_ratio")]
    pub spectral_fullband_ratio: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_db: -38.0,
            ratio: 4.0,
            attack_ms: 4.0,
            hold_ms: 85.0,
            release_ms: 420.0,
            max_reduction_db: -15.0,
            output_gain_db: 0.0,
            preset_name: "Gaming Pro".to_string(),
            spectral_ducking: true,
            voice_sc_filter: true,
            spectral_fullband_ratio: 0.75,
        }
    }
}

impl CompressorConfig {
    pub fn preset_gaming_pro() -> Self {
        Self {
            enabled: true,
            threshold_db: -38.0,
            ratio: 4.0,
            attack_ms: 4.0,
            hold_ms: 85.0,
            release_ms: 420.0,
            max_reduction_db: -15.0,
            output_gain_db: 0.0,
            preset_name: "Gaming Pro".into(),
            spectral_ducking: true,
            voice_sc_filter: true,
            spectral_fullband_ratio: 0.75,
        }
    }

    pub fn preset_competitive() -> Self {
        Self {
            enabled: true,
            threshold_db: -42.0,
            ratio: 6.0,
            attack_ms: 1.5,
            hold_ms: 110.0,
            release_ms: 350.0,
            max_reduction_db: -22.0,
            output_gain_db: 0.0,
            preset_name: "Competitivo".into(),
            spectral_ducking: true,
            voice_sc_filter: true,
            spectral_fullband_ratio: 0.75,
        }
    }

    pub fn preset_music_subtle() -> Self {
        Self {
            enabled: true,
            threshold_db: -34.0,
            ratio: 2.5,
            attack_ms: 10.0,
            hold_ms: 60.0,
            release_ms: 500.0,
            max_reduction_db: -9.0,
            output_gain_db: 0.0,
            preset_name: "Suave / Música".into(),
            spectral_ducking: true,
            voice_sc_filter: true,
            spectral_fullband_ratio: 0.75,
        }
    }

    pub fn preset_obs_classic() -> Self {
        Self {
            enabled: true,
            threshold_db: -50.0,
            ratio: 5.0,
            attack_ms: 1.0,
            hold_ms: 0.0,
            release_ms: 300.0,
            max_reduction_db: -40.0,
            output_gain_db: 0.0,
            preset_name: "OBS Clásico".into(),
            spectral_ducking: false,
            voice_sc_filter: false,
            spectral_fullband_ratio: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: String,
    pub name: String,
    pub exe_name: String,
    #[serde(default)]
    pub device_match: Option<String>,
    pub volume: f32,
    pub muted: bool,
    pub is_trigger: bool,
    pub use_compressor: bool,
    pub exempt_master: bool,
    #[serde(default)]
    pub is_custom: bool,
    #[serde(default)]
    pub voice_clarity: bool,
    #[serde(default)]
    pub is_priority2: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            output_device_id: "default".to_string(),
            show_only_active: false,
            compressor: CompressorConfig::default(),
            voice_clarity_enabled: true,
            voice_clarity_mode: 0,
            deesser_enabled: true,
            noise_gate_enabled: true,
            focus_mode_enabled: true,
            focus_duck_db: -12.0,
            focus_threshold_db: -42.0,
            close_to_tray: true,
            channels: vec![
                ChannelConfig {
                    id: "discord".into(),
                    name: "Discord".into(),
                    exe_name: "Discord.exe".into(),
                    device_match: None,
                    volume: 1.0,
                    muted: false,
                    is_trigger: true,
                    use_compressor: false,
                    exempt_master: true,
                    is_custom: false,
                    voice_clarity: true,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "overwatch".into(),
                    name: "Overwatch".into(),
                    exe_name: "Overwatch.exe".into(),
                    device_match: None,
                    volume: 0.1915,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "chrome".into(),
                    name: "Chrome".into(),
                    exe_name: "chrome.exe".into(),
                    device_match: None,
                    volume: 0.3078,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "wuwa".into(),
                    name: "WuWa".into(),
                    exe_name: "Client-Win64-Shipping.exe".into(),
                    device_match: None,
                    volume: 0.0207,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "zzz".into(),
                    name: "ZZZ".into(),
                    exe_name: "ZenlessZoneZero.exe".into(),
                    device_match: None,
                    volume: 0.0447,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "hi3".into(),
                    name: "HI3".into(),
                    exe_name: "BH3.exe".into(),
                    device_match: None,
                    volume: 0.0246,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "hsr".into(),
                    name: "HSR".into(),
                    exe_name: "StarRail.exe".into(),
                    device_match: None,
                    volume: 0.0127,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "gi".into(),
                    name: "GI".into(),
                    exe_name: "GenshinImpact.exe".into(),
                    device_match: None,
                    volume: 0.1002,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "minecraft".into(),
                    name: "Minecraft".into(),
                    exe_name: "javaw.exe".into(),
                    device_match: None,
                    volume: 0.0374,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "spotify".into(),
                    name: "Spotify".into(),
                    exe_name: "Spotify.exe".into(),
                    device_match: None,
                    volume: 0.0909,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "brave".into(),
                    name: "Brave".into(),
                    exe_name: "brave.exe".into(),
                    device_match: None,
                    volume: 0.0574,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "arknights".into(),
                    name: "Arknights".into(),
                    exe_name: "Endfield.exe".into(),
                    device_match: None,
                    volume: 0.0601,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "nte".into(),
                    name: "NTE".into(),
                    exe_name: "HTGame.exe".into(),
                    device_match: None,
                    volume: 0.0105,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "mumu".into(),
                    name: "Mumu".into(),
                    exe_name: "scrcpy.exe".into(),
                    device_match: None,
                    volume: 1.0,
                    muted: false,
                    is_trigger: false,
                    use_compressor: false,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "opera_gx".into(),
                    name: "Opera GX".into(),
                    exe_name: "opera.exe".into(),
                    device_match: None,
                    volume: 0.1209,
                    muted: false,
                    is_trigger: false,
                    use_compressor: true,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "volum".into(),
                    name: "VoLum".into(),
                    exe_name: "VoLum.exe".into(),
                    device_match: None,
                    volume: 1.0,
                    muted: false,
                    is_trigger: false,
                    use_compressor: false,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
                ChannelConfig {
                    id: "digitech".into(),
                    name: "Digitech".into(),
                    exe_name: "__hardware_input__".into(),
                    device_match: Some("bfc138e0-2398-4437-bc68-caa04d7ba3e6".into()),
                    volume: 0.3543,
                    muted: true, // Muted by default like in user OBS scene
                    is_trigger: false,
                    use_compressor: false,
                    exempt_master: false,
                    is_custom: false,
                    voice_clarity: false,
                    is_priority2: false,
                },
            ],
        }
    }
}

pub fn get_config_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            return parent.join("config.json");
        }
    }
    PathBuf::from("config.json")
}

pub fn load_config() -> AppConfig {
    let path = get_config_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&content) {
                // Ensure Discord has voice clarity enabled if not explicitly toggled off
                for ch in &mut cfg.channels {
                    if ch.id == "discord" || ch.exe_name.to_lowercase() == "discord.exe" {
                        ch.voice_clarity = true;
                    }
                }
                if cfg.compressor.spectral_fullband_ratio < 0.40 {
                    cfg.compressor.spectral_fullband_ratio = 0.75;
                }
                return cfg;
            }
        }
    }
    let default_cfg = AppConfig::default();
    let _ = save_config(&default_cfg);
    default_cfg
}

pub fn save_config(cfg: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let path = get_config_path();
    let json = serde_json::to_string_pretty(cfg)?;
    fs::write(path, json)?;
    Ok(())
}
