/*!
 * Голосовой ввод в реальном времени: запись с микрофона по умолчанию через
 * cpal, локальная транскрибация через whisper-rs. Ничего не уходит с машины.
 *
 * Пока идёт запись, фоновый поток режет звук по паузам тишины (около 2.5с)
 * и шлёт готовые куски текста во фронт событием thuki://voice-chunk, по
 * мере расшифровки. Финальный кусок после нажатия "стоп" приходит отдельным
 * событием с is_final: true. Само решение отправлять сообщение или нет
 * остаётся на фронте, бэкенд только поставляет текст.
 *
 * cpal::Stream не Send на Windows (внутри raw HANDLE), поэтому сам поток
 * живёт на отдельном треде и общается с остальным кодом через AtomicBool
 * и общий буфер сэмплов.
 */

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter, State};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const TARGET_SAMPLE_RATE: u32 = 16000;
const SILENCE_SECS: f64 = 2.5;
const MIN_CHUNK_SECS: f64 = 0.6;
const SILENCE_RMS_THRESHOLD: f32 = 0.015;
const POLL_INTERVAL_MS: u64 = 150;
const VOICE_CHUNK_EVENT: &str = "thuki://voice-chunk";

/// Событие, которое улетает во фронт на каждый расшифрованный кусок.
#[derive(Clone, serde::Serialize)]
struct VoiceChunkPayload {
    text: String,
    is_final: bool,
}

/// Состояние голосового ввода, хранится в app.manage().
pub struct VoiceState {
    model_path: PathBuf,
    whisper_ctx: Mutex<Option<Arc<WhisperContext>>>,
    recording: Mutex<Option<ActiveRecording>>,
}

struct ActiveRecording {
    stop_flag: Arc<AtomicBool>,
    discard_flag: Arc<AtomicBool>,
    capture_handle: JoinHandle<()>,
    processing_handle: JoinHandle<()>,
}

impl VoiceState {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            whisper_ctx: Mutex::new(None),
            recording: Mutex::new(None),
        }
    }

    /// Грузит модель Whisper один раз и переиспользует её на все последующие
    /// записи. Первый вызов может занять секунду-другую, дальше уже быстро.
    fn get_or_load_context(&self) -> Result<Arc<WhisperContext>, String> {
        let mut guard = self.whisper_ctx.lock().map_err(|e| e.to_string())?;
        if let Some(ctx) = guard.as_ref() {
            return Ok(ctx.clone());
        }

        let model_path_str = self
            .model_path
            .to_str()
            .ok_or("некорректный путь к модели whisper")?;

        let ctx = WhisperContext::new_with_params(model_path_str, WhisperContextParameters::default())
            .map_err(|e| format!("не удалось загрузить модель whisper: {e}"))?;
        let ctx = Arc::new(ctx);
        *guard = Some(ctx.clone());
        Ok(ctx)
    }
}

#[tauri::command]
pub async fn start_voice_recording(app: AppHandle, state: State<'_, VoiceState>) -> Result<(), String> {
    {
        let guard = state.recording.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("запись уже идёт".into());
        }
    }

    let ctx = state.get_or_load_context()?;

    let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let discard_flag = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = channel::<u32>();

    let capture_buffer = buffer.clone();
    let capture_stop = stop_flag.clone();
    let capture_handle = std::thread::spawn(move || {
        run_capture_thread(capture_buffer, ready_tx, capture_stop);
    });

    let sample_rate = ready_rx
        .recv()
        .map_err(|_| "не удалось инициализировать запись".to_string())?;

    if sample_rate == 0 {
        stop_flag.store(true, Ordering::SeqCst);
        let _ = capture_handle.join();
        return Err("не найден или недоступен микрофон".into());
    }

    let processing_app = app.clone();
    let processing_buffer = buffer.clone();
    let processing_stop = stop_flag.clone();
    let processing_discard = discard_flag.clone();
    let processing_handle = std::thread::spawn(move || {
        run_processing_thread(
            processing_app,
            ctx,
            processing_buffer,
            sample_rate,
            processing_stop,
            processing_discard,
        );
    });

    let mut guard = state.recording.lock().map_err(|e| e.to_string())?;
    *guard = Some(ActiveRecording {
        stop_flag,
        discard_flag,
        capture_handle,
        processing_handle,
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_voice_recording(state: State<'_, VoiceState>) -> Result<(), String> {
    let recording = {
        let mut guard = state.recording.lock().map_err(|e| e.to_string())?;
        guard.take().ok_or("запись не была начата")?
    };

    recording.stop_flag.store(true, Ordering::SeqCst);

    // Джойним фоновые треды в блокирующем пуле, чтобы не держать executor.
    // Финальный кусок текста придёт отдельным событием, когда досчитается.
    tauri::async_runtime::spawn_blocking(move || {
        let _ = recording.capture_handle.join();
        let _ = recording.processing_handle.join();
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_voice_recording(state: State<'_, VoiceState>) -> Result<(), String> {
    let recording = {
        let mut guard = state.recording.lock().map_err(|e| e.to_string())?;
        guard.take()
    };

    if let Some(recording) = recording {
        recording.discard_flag.store(true, Ordering::SeqCst);
        recording.stop_flag.store(true, Ordering::SeqCst);
        tauri::async_runtime::spawn_blocking(move || {
            let _ = recording.capture_handle.join();
            let _ = recording.processing_handle.join();
        });
    }

    Ok(())
}

/// Открывает микрофон по умолчанию и пишет сэмплы в общий буфер, пока
/// stop_flag не станет true. Живёт на отдельном треде, потому что cpal::Stream
/// не Send.
fn run_capture_thread(buffer: Arc<Mutex<Vec<f32>>>, ready_tx: Sender<u32>, stop_flag: Arc<AtomicBool>) {
    let host = cpal::default_host();

    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            let _ = ready_tx.send(0);
            return;
        }
    };

    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("voice: не удалось получить конфиг микрофона: {e}");
            let _ = ready_tx.send(0);
            return;
        }
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();

    let buffer_clone = buffer.clone();
    let err_fn = |err| eprintln!("voice: ошибка входного потока: {err}");

    let build_result = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| push_samples(&buffer_clone, data, channels),
            err_fn,
            None,
        ),
        other => {
            eprintln!("voice: неподдерживаемый формат сэмплов: {other:?}");
            let _ = ready_tx.send(0);
            return;
        }
    };

    let stream = match build_result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("voice: не удалось создать входной поток: {e}");
            let _ = ready_tx.send(0);
            return;
        }
    };

    if let Err(e) = stream.play() {
        eprintln!("voice: не удалось запустить поток: {e}");
        let _ = ready_tx.send(0);
        return;
    }

    let _ = ready_tx.send(sample_rate);

    while !stop_flag.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    drop(stream);
}

/// Сворачивает многоканальный кадр в моно и складывает в общий буфер.
fn push_samples(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize) {
    let mut buf = match buffer.lock() {
        Ok(b) => b,
        Err(_) => return,
    };

    if channels > 1 {
        for frame in data.chunks(channels) {
            let sum: f32 = frame.iter().sum();
            buf.push(sum / channels as f32);
        }
    } else {
        buf.extend_from_slice(data);
    }
}

/// Следит за буфером, режет речь по паузам и по мере готовности шлёт
/// расшифрованные куски во фронт. После stop_flag добивает хвост буфера
/// и присылает финальный кусок с is_final: true, даже если он пустой,
/// фронту нужен этот сигнал, чтобы сбросить статус в idle.
fn run_processing_thread(
    app: AppHandle,
    ctx: Arc<WhisperContext>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    stop_flag: Arc<AtomicBool>,
    discard_flag: Arc<AtomicBool>,
) {
    let window_samples = ((sample_rate as f64) * 0.15) as usize;
    let mut last_cut: usize = 0;
    let mut voiced_until: usize = 0;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        let stopping = stop_flag.load(Ordering::SeqCst);

        let snapshot_len = buffer.lock().map(|b| b.len()).unwrap_or(last_cut);

        if snapshot_len > voiced_until {
            let tail_start = snapshot_len.saturating_sub(window_samples).max(last_cut);
            let rms = buffer
                .lock()
                .map(|b| rms_of(&b[tail_start..snapshot_len]))
                .unwrap_or(0.0);
            if rms > SILENCE_RMS_THRESHOLD {
                voiced_until = snapshot_len;
            }
        }

        let silence_secs = (snapshot_len.saturating_sub(voiced_until)) as f64 / sample_rate as f64;
        let chunk_secs = (voiced_until.saturating_sub(last_cut)) as f64 / sample_rate as f64;
        let cut_by_silence = silence_secs >= SILENCE_SECS && chunk_secs >= MIN_CHUNK_SECS;

        if cut_by_silence {
            let end = voiced_until;
            let chunk = buffer
                .lock()
                .map(|b| b[last_cut..end].to_vec())
                .unwrap_or_default();
            last_cut = end;

            if !discard_flag.load(Ordering::SeqCst) {
                emit_chunk(&app, &ctx, &chunk, sample_rate, false);
            }
        }

        if stopping {
            break;
        }
    }

    let final_len = buffer.lock().map(|b| b.len()).unwrap_or(last_cut);
    let discarded = discard_flag.load(Ordering::SeqCst);

    if !discarded && final_len > last_cut {
        let tail = buffer
            .lock()
            .map(|b| b[last_cut..final_len].to_vec())
            .unwrap_or_default();
        emit_chunk(&app, &ctx, &tail, sample_rate, true);
    } else {
        let _ = app.emit(
            VOICE_CHUNK_EVENT,
            VoiceChunkPayload {
                text: String::new(),
                is_final: true,
            },
        );
    }
}

/// Расшифровывает один кусок и шлёт результат во фронт. При ошибке
/// финальный кусок всё равно шлётся пустым, чтобы фронт не завис в
/// состоянии "идёт запись".
fn emit_chunk(app: &AppHandle, ctx: &WhisperContext, chunk: &[f32], sample_rate: u32, is_final: bool) {
    match transcribe_chunk(ctx, chunk, sample_rate) {
        Ok(text) => {
            let _ = app.emit(VOICE_CHUNK_EVENT, VoiceChunkPayload { text, is_final });
        }
        Err(e) => {
            eprintln!("voice: ошибка транскрибации: {e}");
            if is_final {
                let _ = app.emit(
                    VOICE_CHUNK_EVENT,
                    VoiceChunkPayload {
                        text: String::new(),
                        is_final: true,
                    },
                );
            }
        }
    }
}

/// Простой линейный ресемплинг в 16кГц, whisper.cpp требует ровно эту частоту.
fn resample_to_16k(samples: &[f32], input_rate: u32) -> Vec<f32> {
    if samples.is_empty() || input_rate == TARGET_SAMPLE_RATE {
        return samples.to_vec();
    }

    let ratio = input_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let out_len = (samples.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        if idx + 1 < samples.len() {
            let frac = (src_pos - idx as f64) as f32;
            out.push(samples[idx] * (1.0 - frac) + samples[idx + 1] * frac);
        } else if idx < samples.len() {
            out.push(samples[idx]);
        }
    }

    out
}

/// Среднеквадратичная амплитуда куска, используется для грубого VAD.
fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Гоняет whisper.cpp на одном куске сэмплов и склеивает текст сегментов.
fn transcribe_chunk(ctx: &WhisperContext, samples: &[f32], sample_rate: u32) -> Result<String, String> {
    if samples.is_empty() {
        return Ok(String::new());
    }

    let resampled = resample_to_16k(samples, sample_rate);

    // Меньше 250мс осмысленно гонять модель нет смысла, скорее всего шум.
    if resampled.len() < (TARGET_SAMPLE_RATE as usize / 4) {
        return Ok(String::new());
    }

    let mut state = ctx.create_state().map_err(|e| e.to_string())?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("auto"));
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_context(true);

    state.full(params, &resampled).map_err(|e| e.to_string())?;

    let mut text = String::new();
    for segment in state.as_iter() {
        text.push_str(&segment.to_string());
    }

    Ok(text.trim().to_string())
}