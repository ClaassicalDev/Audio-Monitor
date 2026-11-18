use crate::config::CompressorConfig;
use std::f32::consts::PI;

pub struct SidechainCompressor {
    pub current_reduction_db: f32,
    hold_samples_remaining: usize,
    sample_rate: f32,
    detector_coeffs: BiquadCoeffs,
    detector_l: BiquadChannel,
    detector_r: BiquadChannel,
}

impl SidechainCompressor {
    pub fn new(sample_rate: f32) -> Self {
        // Voice core bandpass filter (200 Hz - 3800 Hz) to prevent false triggering from desk thumps or mouse clicks
        let detector_coeffs = BiquadCoeffs::bandpass(1200.0, 0.50, sample_rate);
        Self {
            current_reduction_db: 0.0,
            hold_samples_remaining: 0,
            sample_rate,
            detector_coeffs,
            detector_l: BiquadChannel::default(),
            detector_r: BiquadChannel::default(),
        }
    }

    /// Process one trigger stereo frame and return the linear gain multiplier [0.0, 1.0+]
    /// to apply to ducked channels.
    pub fn process_frame(&mut self, trigger_l: f32, trigger_r: f32, cfg: &CompressorConfig) -> f32 {
        if !cfg.enabled {
            self.current_reduction_db = 0.0;
            self.hold_samples_remaining = 0;
            return 1.0;
        }

        // Voice-focused trigger detection: ignore sub-bass thumps and high-frequency clicks if enabled
        let peak = if cfg.voice_sc_filter {
            let fl = self.detector_l.process(trigger_l, &self.detector_coeffs);
            let fr = self.detector_r.process(trigger_r, &self.detector_coeffs);
            fl.abs().max(fr.abs())
        } else {
            trigger_l.abs().max(trigger_r.abs())
        };

        let peak_db = 20.0 * peak.max(1e-6).log10();

        let target_reduction_db = if peak_db > cfg.threshold_db {
            let over_db = peak_db - cfg.threshold_db;
            let ratio = cfg.ratio.max(1.0);
            let raw_reduction = over_db * ((1.0 / ratio) - 1.0); // negative value
            // Enforce max reduction floor (e.g. -15.0 dB prevents ducking deeper than -15.0 dB)
            raw_reduction.max(cfg.max_reduction_db)
        } else {
            0.0
        };

        let attack_ms = cfg.attack_ms.max(0.1);
        let release_ms = cfg.release_ms.max(1.0);

        let alpha_att = (-1.0 / (attack_ms * 0.001 * self.sample_rate)).exp();
        let alpha_rel = (-1.0 / (release_ms * 0.001 * self.sample_rate)).exp();

        if target_reduction_db <= self.current_reduction_db {
            // Attack (voice is speaking / ducking deeper)
            self.current_reduction_db =
                alpha_att * self.current_reduction_db + (1.0 - alpha_att) * target_reduction_db;
            // Reset hold counter whenever compression increases
            self.hold_samples_remaining =
                (cfg.hold_ms.max(0.0) * 0.001 * self.sample_rate).round() as usize;
        } else {
            // Target is higher (voice paused or stopped; wants to return to 0 dB)
            if self.hold_samples_remaining > 0 {
                // Hold phase: lock reduction to prevent pumping between words/syllables
                self.hold_samples_remaining -= 1;
            } else {
                // Release phase: smoothly recover towards target_reduction_db
                self.current_reduction_db =
                    alpha_rel * self.current_reduction_db + (1.0 - alpha_rel) * target_reduction_db;
            }
        }

        // Clamp to prevent runaway
        if self.current_reduction_db > 0.0 {
            self.current_reduction_db = 0.0;
        } else if self.current_reduction_db < -60.0 {
            self.current_reduction_db = -60.0;
        }

        let gain_db = self.current_reduction_db + cfg.output_gain_db;
        10.0_f32.powf(gain_db / 20.0)
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.current_reduction_db = 0.0;
        self.hold_samples_remaining = 0;
        self.detector_l.reset();
        self.detector_r.reset();
    }
}

/// Trackspacer-style Band-Split Spectral Ducking filter.
/// Splits game/music audio into mid vocal band (300 Hz - 4000 Hz) and outer bass/treble band.
/// Applies full ducking to the mid band, but only 35% ducking to bass and treble,
/// keeping sub-bass, explosions, and high air alive and natural while voice speaks!
pub struct SpectralDuckingFilter {
    bpf_coeffs: BiquadCoeffs,
    bpf_l: BiquadChannel,
    bpf_r: BiquadChannel,
}

impl SpectralDuckingFilter {
    pub fn new(sample_rate: f32) -> Self {
        // Centered at 1100 Hz, Q = 0.55 covers ~300 Hz - 4000 Hz (human vocal core)
        let bpf_coeffs = BiquadCoeffs::bandpass(1100.0, 0.55, sample_rate);
        Self {
            bpf_coeffs,
            bpf_l: BiquadChannel::default(),
            bpf_r: BiquadChannel::default(),
        }
    }

    #[inline(always)]
    pub fn process_frame(
        &mut self,
        l: f32,
        r: f32,
        reduction_db: f32,
        spectral_enabled: bool,
        spectral_fullband_ratio: f32,
    ) -> [f32; 2] {
        if reduction_db >= -0.05 {
            return [l, r];
        }

        let g_flat = 10.0_f32.powf(reduction_db / 20.0);

        if !spectral_enabled {
            // Classic flat broadband ducking
            return [l * g_flat, r * g_flat];
        }

        // Mid band extraction (300 Hz - 4 kHz vocal core)
        let mid_l = self.bpf_l.process(l, &self.bpf_coeffs);
        let mid_r = self.bpf_r.process(r, &self.bpf_coeffs);

        // Complementary band (sub-bass < 300 Hz + high treble > 4 kHz)
        let rest_l = l - mid_l;
        let rest_r = r - mid_r;

        // Attenuate mid vocal pocket by 125% of reduction_db (clamped to -45 dB) for an ultra-clean vocal window
        let mid_db = (reduction_db * 1.25).max(-45.0);
        let g_mid = 10.0_f32.powf(mid_db / 20.0);

        // Attenuate rest of spectrum (bass/treble) by spectral_fullband_ratio (default 75% of reduction_db)
        let ratio = spectral_fullband_ratio.clamp(0.40, 1.0);
        let rest_db = reduction_db * ratio;
        let g_rest = 10.0_f32.powf(rest_db / 20.0);

        [mid_l * g_mid + rest_l * g_rest, mid_r * g_mid + rest_r * g_rest]
    }
}

/// Level 2 Focus Mode Ducker (Priority 2)
/// When applications marked as Priority 2 (e.g. Browser, Music player, specific game) emit audio,
/// smoothly ducks all normal channels down to focus_duck_db (default -12.0 dB).
/// Features: 12 ms smooth attack, 350 ms hold (no pumping during pauses/silence in videos/music), 650 ms gentle release.
pub struct FocusDucker {
    pub current_reduction_db: f32,
    hold_samples_remaining: usize,
    sample_rate: f32,
}

impl FocusDucker {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            current_reduction_db: 0.0,
            hold_samples_remaining: 0,
            sample_rate,
        }
    }

    #[inline(always)]
    pub fn process_frame(
        &mut self,
        l: f32,
        r: f32,
        focus_threshold_db: f32,
        focus_duck_db: f32,
    ) -> f32 {
        let peak = l.abs().max(r.abs());
        let peak_db = if peak > 1e-5 {
            20.0 * peak.log10()
        } else {
            -100.0
        };

        let target_reduction_db = if peak_db > focus_threshold_db {
            // Proportional ducking with 4:1 slope, clamped at focus_duck_db
            let over_db = peak_db - focus_threshold_db;
            let reduction = -over_db * 0.75;
            reduction.clamp(focus_duck_db, 0.0)
        } else {
            0.0
        };

        let alpha_att = (-1.0 / (0.012 * self.sample_rate)).exp(); // 12 ms
        let alpha_rel = (-1.0 / (0.650 * self.sample_rate)).exp(); // 650 ms

        if target_reduction_db <= self.current_reduction_db {
            // Attacking (ducking deeper)
            self.current_reduction_db =
                alpha_att * self.current_reduction_db + (1.0 - alpha_att) * target_reduction_db;
            self.hold_samples_remaining = (0.350 * self.sample_rate) as usize; // 350 ms hold
        } else {
            // Target is higher (wants to recover)
            if self.hold_samples_remaining > 0 {
                self.hold_samples_remaining -= 1;
            } else {
                self.current_reduction_db =
                    alpha_rel * self.current_reduction_db + (1.0 - alpha_rel) * target_reduction_db;
            }
        }

        if self.current_reduction_db > 0.0 {
            self.current_reduction_db = 0.0;
        } else if self.current_reduction_db < -45.0 {
            self.current_reduction_db = -45.0;
        }

        self.current_reduction_db
    }

    pub fn reset(&mut self) {
        self.current_reduction_db = 0.0;
        self.hold_samples_remaining = 0;
    }
}

/// Biquad IIR Filter coefficients (normalized with a0 = 1.0)
#[derive(Debug, Clone, Copy)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Default for BiquadCoeffs {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

impl BiquadCoeffs {
    /// 2nd Order Butterworth High-Pass Filter
    pub fn highpass(fc: f32, sample_rate: f32) -> Self {
        let q = 0.70710678f32; // Butterworth Q = 1 / sqrt(2)
        let w0 = 2.0 * PI * fc / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let b0 = (1.0 + cos_w0) * 0.5;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Peaking Bell Parametric EQ Filter
    pub fn peaking(fc: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * PI * fc / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let a_amp = 10.0_f32.powf(gain_db / 40.0);
        let alpha = sin_w0 / (2.0 * q);

        let b0 = 1.0 + alpha * a_amp;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a_amp;
        let a0 = 1.0 + alpha / a_amp;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a_amp;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// High-Shelf EQ Filter
    pub fn highshelf(fc: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * PI * fc / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let a_amp = 10.0_f32.powf(gain_db / 40.0);
        let alpha = sin_w0 / (2.0 * q);
        let two_sqrt_a_alpha = 2.0 * a_amp.sqrt() * alpha;

        let b0 = a_amp * ((a_amp + 1.0) + (a_amp - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = -2.0 * a_amp * ((a_amp - 1.0) + (a_amp + 1.0) * cos_w0);
        let b2 = a_amp * ((a_amp + 1.0) + (a_amp - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a_amp + 1.0) - (a_amp - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a_amp - 1.0) - (a_amp + 1.0) * cos_w0);
        let a2 = (a_amp + 1.0) - (a_amp - 1.0) * cos_w0 - two_sqrt_a_alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Band-Pass Filter (0 dB peak gain at fc)
    pub fn bandpass(fc: f32, q: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * PI * fc / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

/// Direct Form II Transposed Biquad state for one audio channel
#[derive(Debug, Clone, Default)]
pub struct BiquadChannel {
    z1: f32,
    z2: f32,
}

impl BiquadChannel {
    #[inline(always)]
    pub fn process(&mut self, x: f32, coeffs: &BiquadCoeffs) -> f32 {
        let y = coeffs.b0 * x + self.z1;
        self.z1 = coeffs.b1 * x - coeffs.a1 * y + self.z2;
        self.z2 = coeffs.b2 * x - coeffs.a2 * y;
        y
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Studio Voice Clarity DSP Processor:
/// - Downward Expander / Noise Gate (cleans background hiss/fans when quiet)
/// - 4 cascaded Biquad EQ filters (HPF, Anti-Mud Peaking, Presence Peaking, High-Shelf Air)
/// - Dynamic Split-Band De-Esser at 6.2 kHz (tames piercing "S" and "CH" sibilants)
/// - Vocal Dynamic Leveler (compresses peaks, boosts quiet words)
/// - Soft Knee Limiter (tames shouts with zero distortion)
pub struct VoiceClarityProcessor {
    sample_rate: f32,
    mode: u8,
    // Cached coefficients
    hpf_coeffs: BiquadCoeffs,
    eq_mud_coeffs: BiquadCoeffs,
    eq_pres_coeffs: BiquadCoeffs,
    eq_air_coeffs: BiquadCoeffs,
    makeup_gain_db: f32,
    // Filter states for Stereo L & R
    hpf_l: BiquadChannel,
    hpf_r: BiquadChannel,
    mud_l: BiquadChannel,
    mud_r: BiquadChannel,
    pres_l: BiquadChannel,
    pres_r: BiquadChannel,
    air_l: BiquadChannel,
    air_r: BiquadChannel,
    // Dynamic leveler state
    leveler_gain_linear: f32,
    // Downward Expander / Noise Gate state
    gate_gain_linear: f32,
    // De-Esser coefficients and state
    deesser_coeffs: BiquadCoeffs,
    sib_l: BiquadChannel,
    sib_r: BiquadChannel,
    deesser_gain_linear: f32,
}

impl VoiceClarityProcessor {
    pub fn new(sample_rate: f32, initial_mode: u8) -> Self {
        let deesser_coeffs = BiquadCoeffs::bandpass(6200.0, 0.80, sample_rate);
        let mut proc = Self {
            sample_rate,
            mode: 255, // force update
            hpf_coeffs: BiquadCoeffs::default(),
            eq_mud_coeffs: BiquadCoeffs::default(),
            eq_pres_coeffs: BiquadCoeffs::default(),
            eq_air_coeffs: BiquadCoeffs::default(),
            makeup_gain_db: 2.5,
            hpf_l: BiquadChannel::default(),
            hpf_r: BiquadChannel::default(),
            mud_l: BiquadChannel::default(),
            mud_r: BiquadChannel::default(),
            pres_l: BiquadChannel::default(),
            pres_r: BiquadChannel::default(),
            air_l: BiquadChannel::default(),
            air_r: BiquadChannel::default(),
            leveler_gain_linear: 1.0,
            gate_gain_linear: 1.0,
            deesser_coeffs,
            sib_l: BiquadChannel::default(),
            sib_r: BiquadChannel::default(),
            deesser_gain_linear: 1.0,
        };
        proc.set_mode(initial_mode);
        proc
    }

    pub fn set_mode(&mut self, mode: u8) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        let fs = self.sample_rate;

        match mode {
            1 => {
                // Modo 1: Extra Claridad (Anti-Murmullos)
                // - HPF a 120 Hz para máxima limpieza
                // - Peaking 250 Hz, Q 1.2, -4.0 dB
                // - Peaking 3400 Hz, Q 1.1, +5.5 dB para articulación cristalina
                // - High-Shelf 7000 Hz, Q 0.707, +2.5 dB
                // - Nivelador makeup: +3.5 dB
                self.hpf_coeffs = BiquadCoeffs::highpass(120.0, fs);
                self.eq_mud_coeffs = BiquadCoeffs::peaking(250.0, 1.2, -4.0, fs);
                self.eq_pres_coeffs = BiquadCoeffs::peaking(3400.0, 1.1, 5.5, fs);
                self.eq_air_coeffs = BiquadCoeffs::highshelf(7000.0, 0.707, 2.5, fs);
                self.makeup_gain_db = 3.5;
            }
            2 => {
                // Modo 2: Broadcast / Estudio (Cálido y Firme)
                // - HPF a 80 Hz
                // - Peaking 140 Hz, Q 1.4, +1.5 dB (cuerpo y calidez)
                // - Peaking 280 Hz, Q 1.2, -2.5 dB (limpieza de resonancia)
                // - Peaking 3200 Hz, Q 1.0, +3.2 dB
                // - Nivelador makeup: +3.0 dB
                self.hpf_coeffs = BiquadCoeffs::highpass(80.0, fs);
                self.eq_mud_coeffs = BiquadCoeffs::peaking(140.0, 1.4, 1.5, fs);
                self.eq_pres_coeffs = BiquadCoeffs::peaking(3200.0, 1.0, 3.2, fs);
                self.eq_air_coeffs = BiquadCoeffs::highshelf(6500.0, 0.707, 1.8, fs);
                self.makeup_gain_db = 3.0;
            }
            _ => {
                // Modo 0 (Default): Nítido / Presencia (Recomendado)
                // - HPF a 95 Hz (elimina retumbos y golpes sin perder cuerpo)
                // - Peaking 240 Hz, Q 1.2, -3.0 dB (elimina el sonido encajonado de diadema)
                // - Peaking 3200 Hz, Q 1.0, +4.0 dB (inteligibilidad y consonantes claras)
                // - High-Shelf 6500 Hz, Q 0.707, +1.8 dB (aire y definición)
                // - Nivelador makeup: +2.5 dB
                self.hpf_coeffs = BiquadCoeffs::highpass(95.0, fs);
                self.eq_mud_coeffs = BiquadCoeffs::peaking(240.0, 1.2, -3.0, fs);
                self.eq_pres_coeffs = BiquadCoeffs::peaking(3200.0, 1.0, 4.0, fs);
                self.eq_air_coeffs = BiquadCoeffs::highshelf(6500.0, 0.707, 1.8, fs);
                self.makeup_gain_db = 2.5;
            }
        }
    }

    /// Process one stereo sample frame through the complete Voice Clarity chain
    #[inline]
    pub fn process_frame(
        &mut self,
        l: f32,
        r: f32,
        mode: u8,
        deesser_enabled: bool,
        noise_gate_enabled: bool,
    ) -> [f32; 2] {
        if self.mode != mode {
            self.set_mode(mode);
        }

        // 0. Downward Expander / Noise Gate (cleans background hiss and fan noise when no one speaks)
        let (mut sl, mut sr) = if noise_gate_enabled {
            let in_peak = l.abs().max(r.abs());
            let in_db = 20.0 * in_peak.max(1e-6).log10();
            let gate_thresh = -46.0f32; // dB
            let target_gate_db = if in_db < gate_thresh {
                let below = gate_thresh - in_db;
                (-below * (2.5 - 1.0)).max(-14.0) // gentle -14 dB floor
            } else {
                0.0
            };
            let target_gate_linear = 10.0_f32.powf(target_gate_db / 20.0);
            let alpha = if target_gate_linear < self.gate_gain_linear {
                (-1.0 / (0.180 * self.sample_rate)).exp() // close in 180 ms
            } else {
                (-1.0 / (0.002 * self.sample_rate)).exp() // open fast in 2 ms
            };
            self.gate_gain_linear =
                alpha * self.gate_gain_linear + (1.0 - alpha) * target_gate_linear;
            (l * self.gate_gain_linear, r * self.gate_gain_linear)
        } else {
            (l, r)
        };

        // 1. High-Pass Filter (cut sub-bass rumble)
        sl = self.hpf_l.process(sl, &self.hpf_coeffs);
        sr = self.hpf_r.process(sr, &self.hpf_coeffs);

        // 2. Anti-mud Peaking EQ (remove boxy resonance)
        sl = self.mud_l.process(sl, &self.eq_mud_coeffs);
        sr = self.mud_r.process(sr, &self.eq_mud_coeffs);

        // 3. Vocal Presence Peaking EQ (crisp speech articulation)
        sl = self.pres_l.process(sl, &self.eq_pres_coeffs);
        sr = self.pres_r.process(sr, &self.eq_pres_coeffs);

        // 4. High-Shelf Air EQ (definition and clarity)
        sl = self.air_l.process(sl, &self.eq_air_coeffs);
        sr = self.air_r.process(sr, &self.eq_air_coeffs);

        // 5. Dynamic Split-Band De-Esser at 6.2 kHz
        if deesser_enabled {
            let sib_l = self.sib_l.process(sl, &self.deesser_coeffs);
            let sib_r = self.sib_r.process(sr, &self.deesser_coeffs);
            let body_l = sl - sib_l;
            let body_r = sr - sib_r;

            let sib_peak = sib_l.abs().max(sib_r.abs());
            let sib_db = 20.0 * sib_peak.max(1e-6).log10();
            let target_deess_db = if sib_db > -18.0 {
                let over = sib_db - (-18.0);
                over * (0.25 - 1.0) // 4:1 ratio on harsh sibilance
            } else {
                0.0
            };
            let target_deess_linear = 10.0_f32.powf(target_deess_db / 20.0);
            let alpha = if target_deess_linear < self.deesser_gain_linear {
                (-1.0 / (0.0005 * self.sample_rate)).exp() // 0.5 ms ultra-fast attack
            } else {
                (-1.0 / (0.040 * self.sample_rate)).exp() // 40 ms release
            };
            self.deesser_gain_linear =
                alpha * self.deesser_gain_linear + (1.0 - alpha) * target_deess_linear;

            sl = body_l + sib_l * self.deesser_gain_linear;
            sr = body_r + sib_r * self.deesser_gain_linear;
        }

        // 6. Dynamic Vocal Leveler
        // Fast vocal compression: threshold -24.0 dB, ratio 2.5:1, attack 5ms, release 150ms
        let peak = sl.abs().max(sr.abs());
        let peak_db = 20.0 * peak.max(1e-6).log10();

        let target_gain_db = if peak_db > -24.0 {
            let over = peak_db - (-24.0);
            let reduction = over * ((1.0 / 2.5) - 1.0); // negative
            reduction + self.makeup_gain_db
        } else {
            self.makeup_gain_db
        };

        let target_linear = 10.0_f32.powf(target_gain_db / 20.0);

        // Attack 5ms, Release 150ms
        let alpha = if target_linear < self.leveler_gain_linear {
            (-1.0 / (0.005 * self.sample_rate)).exp()
        } else {
            (-1.0 / (0.150 * self.sample_rate)).exp()
        };

        self.leveler_gain_linear = alpha * self.leveler_gain_linear + (1.0 - alpha) * target_linear;

        sl *= self.leveler_gain_linear;
        sr *= self.leveler_gain_linear;

        // 7. Soft Knee Limiter (-0.5 dB ceiling to prevent clipping on loud shouts)
        [soft_limiter(sl), soft_limiter(sr)]
    }
}

/// Soft limiter with transparent knee approaching -0.5 dB (0.944)
#[inline(always)]
pub fn soft_limiter(x: f32) -> f32 {
    let limit = 0.944f32; // -0.5 dB
    let abs_x = x.abs();
    if abs_x <= limit * 0.7 {
        x
    } else {
        let sign = if x >= 0.0 { 1.0 } else { -1.0 };
        sign * (abs_x / limit).tanh() * limit
    }
}

/// Computes peak dB from stereo frame samples
pub fn calculate_peak_db(frames: &[[f32; 2]]) -> f32 {
    let mut max_peak: f32 = 0.0;
    for frame in frames {
        max_peak = max_peak.max(frame[0].abs()).max(frame[1].abs());
    }
    if max_peak < 1e-5 {
        -100.0
    } else {
        20.0 * max_peak.log10()
    }
}

/// Cubic volume slider mapping (OBS math):
/// Slider position [0.0, 1.0] -> Linear volume [0.0, 1.0] = s^3
pub fn slider_to_volume(s: f32) -> f32 {
    let clamped = s.clamp(0.0, 1.5);
    clamped.powi(3)
}

/// Linear volume [0.0, 1.0+] -> Slider position [0.0, 1.0+] = volume^(1/3)
pub fn volume_to_slider(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else {
        v.cbrt().clamp(0.0, 1.5)
    }
}

/// Convert linear gain to dB text
pub fn volume_to_db(v: f32) -> f32 {
    if v <= 1e-5 {
        -100.0
    } else {
        20.0 * v.log10()
    }
}

/// Soft-clip master audio output smoothly at +/- 1.0 to eliminate digital clipping
pub fn soft_clip(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x < -1.0 {
        -1.0
    } else {
        x - (x.powi(3) / 6.0)
    }
}
