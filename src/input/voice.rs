//! Voice input module — microphone capture + Groq Whisper speech-to-text
//!
//! Records audio in 3-second streaming chunks and sends each to the Groq Whisper
//! API for transcription. Transcribed text is fed directly into the active terminal.
//!
//! Activate with the configured voiceInput hotkey (default: Ctrl+A → A).
//! Cancel with Escape.
//!
//! Settings required in settings.json external array:
//!   { "name": "stt", "apiKey": "<groq-api-key>",
//!     "url": "https://api.groq.com/openai/v1/audio/transcriptions" }

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

/// Current state of voice input
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceState {
    Idle,
    Recording,
    Transcribing,
}

/// Result returned by `VoiceInputManager::poll_result`
pub enum VoicePollResult {
    /// New transcribed text is available
    Text(String),
    /// Still recording/transcribing, no new text yet
    Pending,
    /// All transcription finished, transitioning back to Idle
    Done,
    /// Voice input was never started
    Idle,
}

/// Manages voice input: recording, streaming STT, and lifecycle
pub struct VoiceInputManager {
    pub state: VoiceState,
    result_rx: Option<Receiver<String>>,
    stop_tx: Option<Sender<()>>,
}

impl VoiceInputManager {
    pub fn new() -> Self {
        Self {
            state: VoiceState::Idle,
            result_rx: None,
            stop_tx: None,
        }
    }

    /// Start recording.  `api_key`, `api_url`, and `lang` come from the "stt" external vendor.
    pub fn start_recording(&mut self, api_key: String, api_url: String, lang: Option<String>) {
        if !matches!(self.state, VoiceState::Idle) {
            return;
        }

        let (result_tx, result_rx) = std::sync::mpsc::channel::<String>();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();

        self.result_rx = Some(result_rx);
        self.stop_tx = Some(stop_tx);
        self.state = VoiceState::Recording;

        thread::spawn(move || {
            run_recording(result_tx, stop_rx, api_key, api_url, lang);
        });
    }

    /// Signal the recording thread to stop, transition to Transcribing while
    /// the final audio chunk is sent to the API.
    pub fn stop_recording(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if matches!(self.state, VoiceState::Recording) {
            self.state = VoiceState::Transcribing;
        }
    }

    /// Non-blocking poll.  Call this each main-loop iteration.
    pub fn poll_result(&mut self) -> VoicePollResult {
        if matches!(self.state, VoiceState::Idle) {
            return VoicePollResult::Idle;
        }

        let rx = match &self.result_rx {
            Some(r) => r,
            None => {
                self.state = VoiceState::Idle;
                return VoicePollResult::Done;
            }
        };

        match rx.try_recv() {
            Ok(text) => VoicePollResult::Text(text),
            Err(TryRecvError::Empty) => VoicePollResult::Pending,
            Err(TryRecvError::Disconnected) => {
                self.result_rx = None;
                self.state = VoiceState::Idle;
                VoicePollResult::Done
            }
        }
    }

    /// True while any voice activity is in progress
    pub fn is_active(&self) -> bool {
        !matches!(self.state, VoiceState::Idle)
    }

    /// True while the microphone is open
    pub fn is_recording(&self) -> bool {
        matches!(self.state, VoiceState::Recording)
    }

    /// True while waiting for the last STT response
    pub fn is_transcribing(&self) -> bool {
        matches!(self.state, VoiceState::Transcribing)
    }
}

// ─────────────────────────────── recording thread ────────────────────────────

fn run_recording(result_tx: Sender<String>, stop_rx: Receiver<()>, api_key: String, api_url: String, lang: Option<String>) {
    if api_key.is_empty() || api_key == "your-api-key" {
        eprintln!("[VOICE] STT API key not configured. Set your Groq key in settings.json.");
        return;
    }

    let host = cpal::default_host();
    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            eprintln!("[VOICE] No audio input device available");
            return;
        }
    };

    let supported_config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[VOICE] Cannot get default audio config: {}", e);
            return;
        }
    };

    let sample_rate = supported_config.sample_rate();
    let channels = supported_config.channels();
    let sample_format = supported_config.sample_format();
    let stream_config: cpal::StreamConfig = supported_config.into();

    // Samples produced by the cpal callback are sent here
    let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(200);

    let stream = match build_input_stream(&device, &stream_config, sample_format, audio_tx) {
        Some(s) => s,
        None => {
            eprintln!("[VOICE] Could not open audio stream");
            return;
        }
    };

    if let Err(e) = stream.play() {
        eprintln!("[VOICE] Failed to start audio stream: {}", e);
        return;
    }

    eprintln!("[VOICE] Recording started ({} Hz, {} ch, {:?})", sample_rate, channels, sample_format);

    // Streaming: send a chunk to Groq every 3 seconds
    let chunk_interval = Duration::from_secs(3);
    // Minimum 0.25 s of audio before sending (avoid empty / noise-only calls)
    let min_chunk_samples = (sample_rate as usize * channels as usize) / 4;

    let mut accumulated: Vec<f32> = Vec::new();
    let mut last_chunk_time = Instant::now();

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        // Drain all available samples from cpal
        while let Ok(chunk) = audio_rx.try_recv() {
            accumulated.extend_from_slice(&chunk);
        }

        // Every 3 s: spawn a transcription thread with the buffered audio
        if last_chunk_time.elapsed() >= chunk_interval && accumulated.len() >= min_chunk_samples {
            let chunk = std::mem::take(&mut accumulated);
            let tx = result_tx.clone();
            let key = api_key.clone();
            let url = api_url.clone();
            let l = lang.clone();
            thread::spawn(move || transcribe_chunk(chunk, sample_rate, channels, tx, key, url, l));
            last_chunk_time = Instant::now();
        }

        thread::sleep(Duration::from_millis(50));
    }

    // Drain anything that arrived right before the stop signal
    while let Ok(chunk) = audio_rx.try_recv() {
        accumulated.extend_from_slice(&chunk);
    }

    eprintln!("[VOICE] Recording stopped, transcribing final chunk ({} samples)", accumulated.len());

    // Transcribe the remaining audio synchronously so result_tx is dropped only
    // after the final response, giving VoiceInputManager a clean Done transition.
    if accumulated.len() >= min_chunk_samples {
        transcribe_chunk(accumulated, sample_rate, channels, result_tx, api_key, api_url, lang);
    }

    eprintln!("[VOICE] Recording thread done");
}

/// Build a cpal input stream for F32 or I16 formats, converting to Vec<f32>
fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    audio_tx: SyncSender<Vec<f32>>,
) -> Option<cpal::Stream> {
    let tx_f32 = audio_tx.clone();
    let tx_i16 = audio_tx;

    let result = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let _ = tx_f32.try_send(data.to_vec());
            },
            |e| eprintln!("[VOICE] Audio stream error: {}", e),
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let floats: Vec<f32> = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                let _ = tx_i16.try_send(floats);
            },
            |e| eprintln!("[VOICE] Audio stream error: {}", e),
            None,
        ),
        other => {
            eprintln!("[VOICE] Unsupported audio sample format: {:?}", other);
            return None;
        }
    };

    match result {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[VOICE] build_input_stream failed: {}", e);
            None
        }
    }
}

// ──────────────────────────────── transcription ───────────────────────────────

/// Encode, send to Groq, and forward the result text.
fn transcribe_chunk(samples: Vec<f32>, sample_rate: u32, channels: u16, result_tx: Sender<String>, api_key: String, api_url: String, lang: Option<String>) {
    // Mix stereo → mono (Whisper prefers mono)
    let mono: Vec<f32> = if channels > 1 {
        let ch = channels as usize;
        samples.chunks(ch).map(|c| c.iter().sum::<f32>() / ch as f32).collect()
    } else {
        samples
    };

    let wav = encode_wav_i16(&mono, sample_rate);

    match call_groq_stt(wav, &api_key, &api_url, lang.as_deref()) {
        Ok(text) => {
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                let _ = result_tx.send(trimmed);
            }
        }
        Err(e) => eprintln!("[VOICE] Transcription error: {}", e),
    }
}

/// Encode mono f32 samples as a 16-bit PCM WAV byte vector
fn encode_wav_i16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    const CHANNELS: u16 = 1;
    const BITS: u16 = 16;
    let byte_rate = sample_rate * CHANNELS as u32 * (BITS / 8) as u32;
    let block_align = CHANNELS * (BITS / 8);
    let data_size = (samples.len() * 2) as u32;
    let chunk_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + samples.len() * 2);

    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&chunk_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat = PCM
    wav.extend_from_slice(&CHANNELS.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&BITS.to_le_bytes());

    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for &s in samples {
        let pcm = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        wav.extend_from_slice(&pcm.to_le_bytes());
    }

    wav
}

/// POST WAV audio to the Groq transcription endpoint
fn call_groq_stt(wav_bytes: Vec<u8>, api_key: &str, api_url: &str, lang: Option<&str>) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let file_part = reqwest::blocking::multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;

    let mut form = reqwest::blocking::multipart::Form::new()
        .part("file", file_part)
        .text("model", "whisper-large-v3")
        .text("response_format", "verbose_json");

    if let Some(language) = lang {
        form = form.text("language", language.to_string());
    }

    let response = client
        .post(api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = response.json().map_err(|e| e.to_string())?;

    if let Some(err) = json.get("error") {
        return Err(format!("Groq API error: {}", err));
    }

    Ok(json["text"].as_str().unwrap_or("").to_string())
}
