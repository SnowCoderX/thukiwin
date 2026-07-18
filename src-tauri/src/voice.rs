/*!
 * Голосовой ввод в реальном времени: запись с микрофона по умолчанию через
 * cpal, локальная транскрибация через whisper-rs. Ничего не уходит с машины.
 *
 * Пока идёт запись, фоновый поток режет звук по паузам тишины (~1.0с) и шлёт
 * готовые куски текста во фронт событием thuki://voice-chunk, по мере
 * расшифровки. Финальный кусок после нажатия "стоп" приходит отдельным
 * событием с is_final: true.
 */

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter, State};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const TARGET_SAMPLE_RATE: u32 = 16000;
const SILENCE_SECS: f64 = 1.0;
const MIN_CHUNK_SECS: f64 = 0.6;
/// Порог тишины: для нормализованного f32 (-1..1) тихий голос ~0.003-0.008,
/// нормальный ~0.01-0.05. 0.005 ловит шёпот, не ловит фоновый шум.
const SILENCE_RMS_THRESHOLD: f32 = 0.005;
const POLL_INTERVAL_MS: u64 = 150;
const VOICE_CHUNK_EVENT: &str = "thuki://voice-chunk";
const VOICE_LEVEL_EVENT: &str = "thuki://voice-level";

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Событие, которое улетает во фронт на каждый расшифрованный кусок.
#[derive(Clone, serde::Serialize)]
struct VoiceChunkPayload {
    session_id: u64,
    text: String,
    is_final: bool,
}

/// Событие с текущим уровнем громкости для анимации индикатора микрофона.
#[derive(Clone, serde::Serialize)]
struct VoiceLevelPayload {
    session_id: u64,
    level: f32,
}

/// Состояние голосового ввода, хранится в app.manage().
pub struct VoiceState {
    model_path: PathBuf,
    whisper_ctx: Arc<Mutex<Option<Arc<WhisperContext>>>>,
    recording: Arc<Mutex<Option<ActiveRecording>>>,
}

struct ActiveRecording {
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    discard_flag: Arc<AtomicBool>,
    capture_handle: JoinHandle<()>,
    processing_handle: JoinHandle<()>,
}

impl Clone for VoiceState {
    fn clone(&self) -> Self {
        Self {
            model_path: self.model_path.clone(),
            whisper_ctx: Arc::clone(&self.whisper_ctx),
            recording: Arc::clone(&self.recording),
        }
    }
}

impl VoiceState {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            whisper_ctx: Arc::new(Mutex::new(None)),
            recording: Arc::new(Mutex::new(None)),
        }
    }

    /// Грузит модель Whisper один раз и переиспользует её на все последующие
    /// записи. Первый вызов может занять секунду-другую, дальше уже быстро.
    pub fn get_or_load_context(&self) -> Result<Arc<WhisperContext>, String> {
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
        eprintln!("voice: модель whisper загружена ({})", model_path_str);
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
    let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst);
    eprintln!("voice: начинаем сессию #{}", session_id);

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

    eprintln!("voice: микрофон инициализирован, sample_rate={}", sample_rate);

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
            session_id,
        );
    });

    let mut guard = state.recording.lock().map_err(|e| e.to_string())?;
    *guard = Some(ActiveRecording {
        session_id,
        stop_flag,
        discard_flag,
        capture_handle,
        processing_handle,
    });

    eprintln!("voice: сессия #{} активна", session_id);
    Ok(())
}

#[tauri::command]
pub async fn stop_voice_recording(state: State<'_, VoiceState>) -> Result<(), String> {
    let recording = {
        let mut guard = state.recording.lock().map_err(|e| e.to_string())?;
        guard.take()
    };

    if let Some(recording) = recording {
        eprintln!("voice: stop сессии #{}", recording.session_id);
        recording.stop_flag.store(true, Ordering::SeqCst);
        tauri::async_runtime::spawn_blocking(move || {
            let _ = recording.capture_handle.join();
            let _ = recording.processing_handle.join();
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn cancel_voice_recording(state: State<'_, VoiceState>) -> Result<(), String> {
    let recording = {
        let mut guard = state.recording.lock().map_err(|e| e.to_string())?;
        guard.take()
    };

    if let Some(recording) = recording {
        eprintln!("voice: cancel сессии #{}", recording.session_id);
        recording.discard_flag.store(true, Ordering::SeqCst);
        recording.stop_flag.store(true, Ordering::SeqCst);
        tauri::async_runtime::spawn_blocking(move || {
            let _ = recording.capture_handle.join();
            let _ = recording.processing_handle.join();
        });
    }

    Ok(())
}

/// Конвертирует i16 сэмплы в f32 (-1.0..1.0).
fn i16_to_f32(data: &[i16]) -> Vec<f32> {
    data.iter().map(|&s| s as f32 / i16::MAX as f32).collect()
}

/// Конвертирует u16 сэмплы в f32 (-1.0..1.0).
fn u16_to_f32(data: &[u16]) -> Vec<f32> {
    data.iter().map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0).collect()
}

/// Конвертирует i32 сэмплы в f32 (-1.0..1.0).
fn i32_to_f32(data: &[i32]) -> Vec<f32> {
    data.iter().map(|&s| s as f32 / i32::MAX as f32).collect()
}

/// Конвертирует u32 сэмплы в f32 (-1.0..1.0).
fn u32_to_f32(data: &[u32]) -> Vec<f32> {
    data.iter().map(|&s| (s as f32 / u32::MAX as f32) * 2.0 - 1.0).collect()
}

fn run_capture_thread(buffer: Arc<Mutex<Vec<f32>>>, ready_tx: Sender<u32>, stop_flag: Arc<AtomicBool>) {
    let host = cpal::default_host();

    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            eprintln!("voice: default_input_device не найден");
            let _ = ready_tx.send(0);
            return;
        }
    };

    let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
    eprintln!("voice: используем микрофон: {}", device_name);

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
    eprintln!("voice: конфиг микрофона: rate={}Hz, channels={}, format={:?}", sample_rate, channels, sample_format);

    let buffer_clone = buffer.clone();
    let err_fn = |err| eprintln!("voice: ошибка входного потока: {err}");

    // Поддерживаем ВСЕ форматы сэмплов, конвертируя в f32
    let build_result = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| push_samples_f32(&buffer_clone, data, channels),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _| {
                let f32_data = i16_to_f32(data);
                push_samples_f32(&buffer_clone, &f32_data, channels);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _| {
                let f32_data = u16_to_f32(data);
                push_samples_f32(&buffer_clone, &f32_data, channels);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            &config.into(),
            move |data: &[i32], _| {
                let f32_data = i32_to_f32(data);
                push_samples_f32(&buffer_clone, &f32_data, channels);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U32 => device.build_input_stream(
            &config.into(),
            move |data: &[u32], _| {
                let f32_data = u32_to_f32(data);
                push_samples_f32(&buffer_clone, &f32_data, channels);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I8 => device.build_input_stream(
            &config.into(),
            move |data: &[i8], _| {
                let f32_data: Vec<f32> = data.iter().map(|&s| (s as f32) / 128.0).collect();
                push_samples_f32(&buffer_clone, &f32_data, channels);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U8 => device.build_input_stream(
            &config.into(),
            move |data: &[u8], _| {
                let f32_data: Vec<f32> = data.iter().map(|&s| ((s as f32) - 128.0) / 128.0).collect();
                push_samples_f32(&buffer_clone, &f32_data, channels);
            },
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

    eprintln!("voice: поток запущен");
    let _ = ready_tx.send(sample_rate);

    while !stop_flag.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    eprintln!("voice: capture поток завершается");
    drop(stream);
}

fn push_samples_f32(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize) {
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

fn run_processing_thread(
    app: AppHandle,
    ctx: Arc<WhisperContext>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    stop_flag: Arc<AtomicBool>,
    discard_flag: Arc<AtomicBool>,
    session_id: u64,
) {
    let window_samples = ((sample_rate as f64) * 0.15) as usize;
    let mut last_cut: usize = 0;
    let mut voiced_until: usize = 0;
    let mut last_level_emit = std::time::Instant::now();
    let mut total_samples: usize = 0;
    let mut chunks_emitted: usize = 0;
    let mut last_emitted_text: String = String::new();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        let stopping = stop_flag.load(Ordering::SeqCst);

        let snapshot_len = buffer.lock().map(|b| b.len()).unwrap_or(last_cut);
        if snapshot_len > total_samples {
            total_samples = snapshot_len;
        }

        // Эмитим уровень громкости каждые 50мс
        if last_level_emit.elapsed().as_millis() >= 50 {
            last_level_emit = std::time::Instant::now();
            let level = if snapshot_len > window_samples {
                buffer.lock().map(|b| rms_of(&b[snapshot_len - window_samples..snapshot_len])).unwrap_or(0.0)
            } else if snapshot_len > 0 {
                buffer.lock().map(|b| rms_of(&b[..snapshot_len])).unwrap_or(0.0)
            } else {
                0.0
            };
            let _ = app.emit(VOICE_LEVEL_EVENT, VoiceLevelPayload { session_id, level });
        }

        // Обновляем voiced_until если есть звук
        if snapshot_len > voiced_until {
            let tail_start = snapshot_len.saturating_sub(window_samples).max(last_cut);
            let rms = buffer.lock().map(|b| rms_of(&b[tail_start..snapshot_len])).unwrap_or(0.0);
            if rms > SILENCE_RMS_THRESHOLD {
                if voiced_until == 0 || voiced_until < snapshot_len {
                    eprintln!("voice: [#{}] голос обнаружен, rms={:.4}, samples={}", session_id, rms, snapshot_len);
                }
                voiced_until = snapshot_len;
            }
        }

        let silence_secs = (snapshot_len.saturating_sub(voiced_until)) as f64 / sample_rate as f64;
        let chunk_secs = (voiced_until.saturating_sub(last_cut)) as f64 / sample_rate as f64;
        let cut_by_silence = silence_secs >= SILENCE_SECS && chunk_secs >= MIN_CHUNK_SECS;

        if cut_by_silence {
            let end = voiced_until;
            let chunk = buffer.lock().map(|b| b[last_cut..end].to_vec()).unwrap_or_default();
            eprintln!("voice: [#{}] нарезаем чанк: {} samples ({}s)", session_id, chunk.len(), chunk_secs);
            last_cut = end;

            if !discard_flag.load(Ordering::SeqCst) {
                if let Ok(text) = transcribe_chunk(&ctx, &chunk, sample_rate) {
                    if !text.is_empty() && text != last_emitted_text {
                        last_emitted_text = text.clone();
                        let _ = app.emit(VOICE_CHUNK_EVENT, VoiceChunkPayload { session_id, text, is_final: false });
                        chunks_emitted += 1;
                    }
                }
            }
        }

        if stopping {
            break;
        }
    }

    let final_len = buffer.lock().map(|b| b.len()).unwrap_or(last_cut);
    let discarded = discard_flag.load(Ordering::SeqCst);
    let tail_secs = (final_len.saturating_sub(last_cut)) as f64 / sample_rate as f64;

    eprintln!("voice: [#{}] остановка. total_samples={}, chunks={}, discarded={}, tail={:.2}s",
              session_id, total_samples, chunks_emitted, discarded, tail_secs);

    if discarded {
        eprintln!("voice: [#{}] отправляем пустой финальный чанк (discarded)", session_id);
        let _ = app.emit(
            VOICE_CHUNK_EVENT,
            VoiceChunkPayload {
                session_id,
                text: String::new(),
                is_final: true,
            },
        );
        return;
    }

    // При остановке (stop/cancel) транскрибируем ВСЁ накопленное аудио как один чанк,
    // а не только хвост после last_cut. Это решает проблему обрезания текста
    // когда пользователь отпускает Ctrl раньше 1 секунды тишины.
    if final_len > 0 {
        let full_audio = buffer.lock().map(|b| b[..final_len].to_vec()).unwrap_or_default();
        eprintln!("voice: [#{}] финальная транскрибация: {} samples total", session_id, full_audio.len());
        match transcribe_chunk(&ctx, &full_audio, sample_rate) {
            Ok(text) => {
                let final_text = if text == last_emitted_text {
                    // Если полная транскрибация дала тот же результат что и последний чанк —
                    // значит новых слов не добавилось, отправляем пустой is_final
                    String::new()
                } else {
                    text
                };
                eprintln!("voice: [#{}] финальный результат: '{}'", session_id, final_text);
                let _ = app.emit(VOICE_CHUNK_EVENT, VoiceChunkPayload { session_id, text: final_text, is_final: true });
            }
            Err(e) => {
                eprintln!("voice: [#{}] ошибка финальной транскрибации: {e}", session_id);
                let _ = app.emit(VOICE_CHUNK_EVENT, VoiceChunkPayload { session_id, text: String::new(), is_final: true });
            }
        }
    } else {
        eprintln!("voice: [#{}] отправляем пустой финальный чанк (нет аудио)", session_id);
        let _ = app.emit(
            VOICE_CHUNK_EVENT,
            VoiceChunkPayload {
                session_id,
                text: String::new(),
                is_final: true,
            },
        );
    }
}

fn emit_chunk(
    app: &AppHandle,
    ctx: &WhisperContext,
    chunk: &[f32],
    sample_rate: u32,
    is_final: bool,
    session_id: u64,
) {
    let start = std::time::Instant::now();
    match transcribe_chunk(ctx, chunk, sample_rate) {
        Ok(text) => {
            eprintln!(
                "voice: [#{}] чанк {} мс -> '{}' (транскрибация заняла {:?})",
                session_id,
                chunk.len() * 1000 / sample_rate as usize,
                text,
                start.elapsed()
            );
            let _ = app.emit(VOICE_CHUNK_EVENT, VoiceChunkPayload { session_id, text, is_final });
        }
        Err(e) => {
            eprintln!("voice: [#{}] ошибка транскрибации: {e}", session_id);
            if is_final {
                let _ = app.emit(
                    VOICE_CHUNK_EVENT,
                    VoiceChunkPayload {
                        session_id,
                        text: String::new(),
                        is_final: true,
                    },
                );
            }
        }
    }
}

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

fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Гоняет whisper.cpp на одном куске сэмплов. Создаёт свой WhisperState
/// для каждого чанка — это медленнее, но гарантирует отсутствие "памяти"
/// между чанками и устраняет дублирование текста.
/// Список фраз-галлюцинаций, которые whisper любит добавлять в конце.
/// Проверяем в нижнем регистре, удаляем в оригинальном.
static HALLUCINATION_PATTERNS: &[&str] = &[
    "Thank you",
    "thank you",
    "THANK YOU",
    "Thanks",
    "thanks",
    "Thank you very much",
    "Thank you.",
    "thank you.",
    "Thanks for watching",
    "Subtitles by",
    "Please subscribe",
];

/// Убирает типичные whisper-галлюцинации с конца текста.
fn strip_hallucinations(text: &str) -> String {
    let mut result = text.trim().to_string();
    // Удаляем повторяющиеся вежливые фразы (whisper иногда дублирует)
    for pattern in HALLUCINATION_PATTERNS {
        // Удаляем с конца, включая варианты с пунктуацией
        let lower = result.to_lowercase();
        let pattern_lower = pattern.to_lowercase();
        if lower.ends_with(&pattern_lower) {
            let cutoff = result.len() - pattern.len();
            result = result[..cutoff].trim().to_string();
        }
        // Также проверяем с точкой/запятой в конце
        for suffix in [".", ",", "!", "?"] {
            let with_suffix = format!("{}{}", pattern_lower, suffix);
            if lower.ends_with(&with_suffix) {
                let cutoff = result.len() - with_suffix.len();
                result = result[..cutoff].trim().to_string();
            }
        }
    }
    result
}

/// Распознаёт речь с приоритетом на русский и английский.
/// Стратегия: запускаем auto-detect, но если результат короткий или похож на мусор —
/// пробуем явно ru и en, выбирая лучший.
fn transcribe_chunk(ctx: &WhisperContext, samples: &[f32], sample_rate: u32) -> Result<String, String> {
    if samples.is_empty() {
        return Ok(String::new());
    }

    let resampled = resample_to_16k(samples, sample_rate);
    eprintln!("voice: transcribe_chunk: {} samples -> {} resampled", samples.len(), resampled.len());

    if resampled.len() < (TARGET_SAMPLE_RATE as usize / 4) {
        eprintln!("voice: чанк слишком короткий (<250ms), пропускаем");
        return Ok(String::new());
    }

    // Первый проход: auto-detect
    let text_auto = run_whisper_pass(ctx, &resampled, None)?;
    eprintln!("voice: auto-detect text='{}'", text_auto);

    // Эвристика: если auto дал осмысленный результат (достаточно длинный, без подозрительных паттернов) — используем
    let auto_looks_good = text_auto.len() >= 5 && !looks_like_gibberish(&text_auto);
    if auto_looks_good {
        let result = strip_hallucinations(&text_auto);
        eprintln!("voice: result (auto) -> '{}'", result);
        return Ok(result);
    }

    // Auto дал мусор или слишком короткий результат — пробуем ru и en явно
    let text_en = run_whisper_pass(ctx, &resampled, Some("en"))?;
    let text_ru = run_whisper_pass(ctx, &resampled, Some("ru"))?;
    eprintln!("voice: fallback en='{}' ru='{}'", text_en, text_ru);

    // Выбираем тот, у которого результат длиннее и не похож на мусор
    let en_good = !looks_like_gibberish(&text_en);
    let ru_good = !looks_like_gibberish(&text_ru);

    let best = if en_good && ru_good {
        if text_en.len() >= text_ru.len() { text_en } else { text_ru }
    } else if en_good {
        text_en
    } else if ru_good {
        text_ru
    } else {
        // Оба мусор — берём тот что длиннее
        if text_en.len() >= text_ru.len() { text_en } else { text_ru }
    };

    let result = strip_hallucinations(&best);
    eprintln!("voice: result (fallback) -> '{}'", result);
    Ok(result)
}

/// Простая эвристика: проверяет, не похож ли текст на мусор (много повторов одного символа и т.д.)
fn looks_like_gibberish(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    // Если текст состоит из одного символа, повторённого много раз — мусор
    let first_char = text.chars().next().unwrap();
    if text.chars().all(|c| c == first_char) && text.len() > 3 {
        return true;
    }
    // Если нет ни одной буквы — мусор
    if !text.chars().any(|c| c.is_alphabetic()) {
        return true;
    }
    false
}

/// Один проход распознавания. Возвращает текст.
fn run_whisper_pass(
    ctx: &WhisperContext,
    samples: &[f32],
    force_lang: Option<&str>,
) -> Result<String, String> {
    let mut state = ctx.create_state().map_err(|e| e.to_string())?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(force_lang);
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_context(true);
    params.set_n_threads(4);
    params.set_temperature(0.0);
    params.set_temperature_inc(0.0);

    state.full(params, samples).map_err(|e| e.to_string())?;

    let mut text = String::new();
    for segment in state.as_iter() {
        text.push_str(&segment.to_string());
    }

    Ok(text.trim().to_string())
}