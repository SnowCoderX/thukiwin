/*!
 * Голосовой ввод в реальном времени: запись с микрофона по умолчанию через
 * cpal, локальная транскрибация через whisper-rs. Ничего не уходит с машины.
 *
 * Два режима записи:
 * - Ручной (Ctrl зажат): фоновый поток режет звук по паузам тишины (~1.0с) и
 *   шлёт готовые куски текста во фронт событием thuki://voice-chunk по мере
 *   расшифровки — они ложатся в текстовое поле. Финальный кусок после
 *   отпускания Ctrl приходит отдельным событием с is_final: true и пустым
 *   текстом (весь текст уже был отправлен по кускам).
 * - Авто (после wake-word "туки"): вся фраза копится целиком, распознаётся
 *   одним проходом после ~1.2с тишины, и финальное событие содержит is_final:
 *   true с ПОЛНЫМ текстом — фронт сразу вызывает ask(), без участия
 *   текстового поля. Пока идёт тишина после речи, шлётся событие
 *   thuki://voice-countdown с долей (0..1) до авто-отправки — им управляется
 *   кольцо на кнопке отправки в UI.
 *
 * В обоих режимах, пока запись активна, фоновый wake-word слушатель
 * (wakeword.rs) отпускает микрофон (см. `suppressed` в WakeWordState),
 * чтобы не конкурировать за устройство с этим модулем.
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
const VOICE_COUNTDOWN_EVENT: &str = "thuki://voice-countdown";

/// Тишина, после которой авто-сессия (запущенная wake-word'ом) считается
/// законченной и текст уходит в ask() без участия пользователя.
const AUTO_SUBMIT_SILENCE_SECS: f64 = 1.2;

/// Короче обычной — используется, когда вопрос уже целиком пришёл вместе
/// с "туки" (prefix_text не пуст) и новой речи в этой сессии ещё не было:
/// ждать полную секунду незачем, скорее всего, ничего больше не скажут.
const AUTO_SUBMIT_SILENCE_SECS_PREFIX_ONLY: f64 = 0.35;

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

/// Событие с долей (0..1) до авто-отправки — только для авто-сессий,
/// растёт по мере того как длится тишина после последней сказанной фразы.
#[derive(Clone, serde::Serialize)]
struct VoiceCountdownPayload {
    session_id: u64,
    fraction: f64,
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
pub async fn start_voice_recording(
    app: AppHandle,
    state: State<'_, VoiceState>,
    wake_word: State<'_, crate::wakeword::WakeWordState>,
    auto_submit: Option<bool>,
    prefix_text: Option<String>,
) -> Result<(), String> {
    {
        let guard = state.recording.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("запись уже идёт".into());
        }
    }

    let auto_submit = auto_submit.unwrap_or(false);
    let prefix_text = prefix_text.unwrap_or_default();
    let suppressed_flag = wake_word.suppressed.clone();

    let ctx = state.get_or_load_context()?;
    let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst);
    eprintln!(
        "voice: начинаем сессию #{} (auto_submit={})",
        session_id, auto_submit
    );

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

    // Пока идёт запись (ручная или авто), фоновый wake-word слушатель
    // отпускает микрофон — иначе два потока cpal будут конкурировать за устройство.
    suppressed_flag.store(true, Ordering::SeqCst);

    let processing_app = app.clone();
    let processing_buffer = buffer.clone();
    let processing_stop = stop_flag.clone();
    let processing_discard = discard_flag.clone();

    let processing_handle = if auto_submit {
        let auto_suppressed_flag = suppressed_flag.clone();
        let recording_slot = state.recording.clone();
        std::thread::spawn(move || {
            run_auto_submit_processing_thread(
                processing_app,
                ctx,
                processing_buffer,
                sample_rate,
                processing_stop,
                processing_discard,
                session_id,
                prefix_text,
                auto_suppressed_flag,
                recording_slot,
            );
        })
    } else {
        std::thread::spawn(move || {
            run_processing_thread(
                processing_app,
                ctx,
                processing_buffer,
                sample_rate,
                processing_stop,
                processing_discard,
                session_id,
                suppressed_flag,
            );
        })
    };

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
pub(crate) fn i16_to_f32(data: &[i16]) -> Vec<f32> {
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

/// Открывает микрофон по умолчанию и пишет сэмплы (в f32, моно) в `buffer`
/// до сигнала `stop_flag`. Публичный на уровне крейта — переиспользуется
/// фоновым wake-word слушателем (wakeword.rs), чтобы не дублировать код
/// работы с cpal для всех форматов сэмплов.
pub(crate) fn run_capture_thread(buffer: Arc<Mutex<Vec<f32>>>, ready_tx: Sender<u32>, stop_flag: Arc<AtomicBool>) {
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

/// Публичная на уровне крейта — переиспользуется wake-word слушателем.
pub(crate) fn push_samples_f32(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize) {
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

/// Ручной режим (Ctrl зажат): режет речь по паузам ~1.0с, шлёт каждый кусок
/// во фронт по мере расшифровки (ложится в текстовое поле). Не изменился по
/// поведению — только получил `suppressed_flag`, который снимает в конце,
/// возвращая микрофон фоновому wake-word слушателю.
#[allow(clippy::too_many_arguments)]
fn run_processing_thread(
    app: AppHandle,
    ctx: Arc<WhisperContext>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    stop_flag: Arc<AtomicBool>,
    discard_flag: Arc<AtomicBool>,
    session_id: u64,
    suppressed_flag: Arc<AtomicBool>,
) {
    let window_samples = ((sample_rate as f64) * 0.15) as usize;
    let mut last_cut: usize = 0;
    let mut voiced_until: usize = 0;
    let mut last_level_emit = std::time::Instant::now();
    let mut total_samples: usize = 0;
    let mut chunks_emitted: usize = 0;

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
                    if !text.is_empty() {
                        // Не сравниваем с last_emitted_text — whisper с no_context может
                        // давать разные результаты для одного и того же аудио из-за
                        // случайности в сэмплировании (даже при temperature=0).
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

    // Обрабатываем оставшийся аудио после last_cut — это последние слова,
    // сказанные пользователем перед отпусканием Ctrl (без паузы в 1 секунду).
    let remaining = if !discarded && final_len > last_cut {
        let chunk = buffer.lock().map(|b| b[last_cut..final_len].to_vec()).unwrap_or_default();
        eprintln!("voice: [#{}] финальная нарезка: {} samples", session_id, chunk.len());
        chunk
    } else {
        Vec::new()
    };

    if !remaining.is_empty() {
        if let Ok(text) = transcribe_chunk(&ctx, &remaining, sample_rate) {
            if !text.is_empty() {
                let _ = app.emit(VOICE_CHUNK_EVENT, VoiceChunkPayload { session_id, text: text.clone(), is_final: false });
                eprintln!("voice: [#{}] последний чанк отправлен: '{}'", session_id, text);
            }
        }
    }

    // Финальный сигнал — запись закончена, текст пустой (весь текст уже отправлен)
    eprintln!("voice: [#{}] сессия завершена (discarded={})", session_id, discarded);
    let _ = app.emit(
        VOICE_CHUNK_EVENT,
        VoiceChunkPayload {
            session_id,
            text: String::new(),
            is_final: true,
        },
    );

    // Возвращаем микрофон фоновому wake-word слушателю.
    suppressed_flag.store(false, Ordering::SeqCst);
}

/// Авто-режим (запущен wake-word'ом "туки"): не режет на промежуточные
/// куски — копит всю фразу, ждёт `AUTO_SUBMIT_SILENCE_SECS` тишины после
/// последней сказанной части, затем распознаёт ОДНИМ проходом и шлёт полный
/// текст как is_final — фронт сразу отправляет его в ask(), без текстового
/// поля. Параллельно шлёт thuki://voice-countdown с долей (0..1) оставшейся
/// тишины до отправки, чтобы UI мог показать обратный отсчёт на кнопке.
#[allow(clippy::too_many_arguments)]
fn run_auto_submit_processing_thread(
    app: AppHandle,
    ctx: Arc<WhisperContext>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    stop_flag: Arc<AtomicBool>,
    discard_flag: Arc<AtomicBool>,
    session_id: u64,
    prefix_text: String,
    suppressed_flag: Arc<AtomicBool>,
    recording_slot: Arc<Mutex<Option<ActiveRecording>>>,
) {
    let window_samples = ((sample_rate as f64) * 0.15) as usize;
    let mut voiced_until: usize = 0;
    let mut last_countdown_emit = std::time::Instant::now();
    let mut last_level_emit = std::time::Instant::now();

    // Если вся команда уже была сказана вместе с "туки" в одно дыхание, она
    // целиком уехала в prefix_text ещё в wakeword.rs, и в этой сессии может
    // не быть вообще никакой новой речи. Без этого has_speech никогда не
    // станет true и авто-остановка по тишине не сработает никогда.
    let has_prefix = !prefix_text.trim().is_empty();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        let stopping = stop_flag.load(Ordering::SeqCst);

        let snapshot_len = buffer.lock().map(|b| b.len()).unwrap_or(0);

        if last_level_emit.elapsed().as_millis() >= 50 {
            last_level_emit = std::time::Instant::now();
            let level = if snapshot_len > window_samples {
                buffer.lock().map(|b| rms_of(&b[snapshot_len - window_samples..snapshot_len])).unwrap_or(0.0)
            } else {
                0.0
            };
            let _ = app.emit(VOICE_LEVEL_EVENT, VoiceLevelPayload { session_id, level });
        }

        if snapshot_len > voiced_until {
            let tail_start = snapshot_len.saturating_sub(window_samples);
            let rms = buffer.lock().map(|b| rms_of(&b[tail_start..snapshot_len])).unwrap_or(0.0);
            if rms > SILENCE_RMS_THRESHOLD {
                voiced_until = snapshot_len;
            }
        }

        let silence_secs = (snapshot_len.saturating_sub(voiced_until)) as f64 / sample_rate as f64;
        let has_speech = voiced_until >= (sample_rate as f64 * MIN_CHUNK_SECS.min(0.3)) as usize;

        // Обратный отсчёт для кольца на кнопке отправки — растёт только
        // когда есть тишина ПОСЛЕ уже сказанной речи.
        if last_countdown_emit.elapsed().as_millis() >= 100 {
            last_countdown_emit = std::time::Instant::now();
            let fraction = if has_speech {
                (silence_secs / AUTO_SUBMIT_SILENCE_SECS).min(1.0)
            } else {
                0.0
            };
            let _ = app.emit(VOICE_COUNTDOWN_EVENT, VoiceCountdownPayload { session_id, fraction });
        }

        let auto_stop = if has_prefix && !has_speech {
            silence_secs >= AUTO_SUBMIT_SILENCE_SECS_PREFIX_ONLY
        } else {
            (has_speech || has_prefix) && silence_secs >= AUTO_SUBMIT_SILENCE_SECS
        };

        if stopping || auto_stop {
            if auto_stop {
                eprintln!("voice: [#{}] авто-сессия: тишина {:.2}с, завершаем", session_id, silence_secs);
            }
            // Сигналим потоку захвата остановиться. При обычном stop/cancel это
            // уже сделала вызывающая команда, но при само-завершении по тишине
            // (auto_stop) этого никто больше не делает, без этой строки
            // микрофон и cpal-поток продолжат работать в фоне бесконечно.
            stop_flag.store(true, Ordering::SeqCst);
            break;
        }
    }

    let discarded = discard_flag.load(Ordering::SeqCst);
    let final_len = buffer.lock().map(|b| b.len()).unwrap_or(0);
    let end = voiced_until.min(final_len);

    // Если новой речи в этой сессии нет (end == 0), но prefix_text не пуст —
    // значит команда была сказана целиком вместе с "туки" ещё до старта этой
    // сессии (см. wakeword.rs::match_wake_word). Отправляем prefix_text как есть.
    // Если и prefix_text пуст — действительно ничего не было сказано, шлём пусто.
    let combined = if discarded {
        String::new()
    } else if end == 0 {
        // Новой речи в этой сессии нет — либо вся команда уже целиком уехала
        // в prefix_text (сказано одним дыханием вместе с "туки"), либо
        // действительно ничего не сказано (тогда prefix_text тоже пуст).
        prefix_text.trim().to_string()
    } else {
        let audio = buffer.lock().map(|b| b[..end].to_vec()).unwrap_or_default();
        let heard = transcribe_chunk(&ctx, &audio, sample_rate).unwrap_or_default();
        if prefix_text.trim().is_empty() {
            heard
        } else if heard.trim().is_empty() {
            prefix_text.clone()
        } else {
            format!("{} {}", prefix_text.trim(), heard.trim())
        }
    };

    eprintln!("voice: [#{}] авто-сессия завершена, текст='{}'", session_id, combined);

    let _ = app.emit(
        VOICE_CHUNK_EVENT,
        VoiceChunkPayload {
            session_id,
            text: combined,
            is_final: true,
        },
    );

    // Возвращаем микрофон фоновому wake-word слушателю.
    suppressed_flag.store(false, Ordering::SeqCst);

    // Сессия завершилась сама (по тишине), а не через явный stop/cancel,
    // значит никто ещё не убрал её из VoiceState.recording. Без этого
    // следующий start_voice_recording будет вечно получать "запись уже
    // идёт", даже когда на самом деле уже ничего не происходит. Сверяем
    // session_id на случай (маловероятной) гонки с уже новой сессией.
    if let Ok(mut guard) = recording_slot.lock() {
        let is_still_mine = matches!(guard.as_ref(), Some(active) if active.session_id == session_id);
        if is_still_mine {
            *guard = None;
        }
    }
}

/// Публичная на уровне крейта — переиспользуется wake-word слушателем.
pub(crate) fn resample_to_16k(samples: &[f32], input_rate: u32) -> Vec<f32> {
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

/// Публичная на уровне крейта — переиспользуется wake-word слушателем.
pub(crate) fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

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

/// Распознаёт речь с приоритетом на русский и английский.
/// Стратегия:
/// - Для коротких фраз (< 2 сек) — сразу пробуем ru, потом en, без auto-detect
/// - Для длинных фраз — auto-detect с языковым prompt, fallback на ru/en
/// - Используем initial_prompt чтобы подсказать модели ожидаемые языки
pub(crate) fn transcribe_chunk(ctx: &WhisperContext, samples: &[f32], sample_rate: u32) -> Result<String, String> {
    if samples.is_empty() {
        return Ok(String::new());
    }

    let resampled = resample_to_16k(samples, sample_rate);
    eprintln!("voice: transcribe_chunk: {} samples -> {} resampled", samples.len(), resampled.len());

    if resampled.len() < (TARGET_SAMPLE_RATE as usize / 4) {
        eprintln!("voice: чанк слишком короткий (<250ms), пропускаем");
        return Ok(String::new());
    }

    let duration_secs = resampled.len() as f64 / TARGET_SAMPLE_RATE as f64;
    let is_short = duration_secs < 2.0;

    if is_short {
        // Для коротких фраз auto-detect почти всегда гадит — сразу пробуем ru, потом en
        let text_ru = run_whisper_pass(ctx, &resampled, Some("ru"))?;
        let ru_good = !looks_like_gibberish(&text_ru) && !text_ru.is_empty();
        eprintln!("voice: short chunk ru='{}' good={}", text_ru, ru_good);

        if ru_good {
            let result = strip_hallucinations(&text_ru);
            eprintln!("voice: result (short ru) -> '{}'", result);
            return Ok(result);
        }

        let text_en = run_whisper_pass(ctx, &resampled, Some("en"))?;
        let en_good = !looks_like_gibberish(&text_en) && !text_en.is_empty();
        eprintln!("voice: short chunk en='{}' good={}", text_en, en_good);

        if en_good {
            let result = strip_hallucinations(&text_en);
            eprintln!("voice: result (short en) -> '{}'", result);
            return Ok(result);
        }

        // Оба плохие — берём тот что длиннее
        let best = if text_ru.len() >= text_en.len() { text_ru } else { text_en };
        let result = strip_hallucinations(&best);
        eprintln!("voice: result (short fallback) -> '{}'", result);
        return Ok(result);
    }

    // Для длинных фраз: auto-detect с языковым prompt, затем fallback
    let text_auto = run_whisper_pass(ctx, &resampled, None)?;
    eprintln!("voice: auto-detect text='{}'", text_auto);

    let auto_looks_good = text_auto.len() >= 5 && !looks_like_gibberish(&text_auto);
    if auto_looks_good {
        let result = strip_hallucinations(&text_auto);
        eprintln!("voice: result (auto) -> '{}'", result);
        return Ok(result);
    }

    // Auto дал мусор — пробуем ru и en явно
    let text_ru = run_whisper_pass(ctx, &resampled, Some("ru"))?;
    let text_en = run_whisper_pass(ctx, &resampled, Some("en"))?;
    eprintln!("voice: fallback en='{}' ru='{}'", text_en, text_ru);

    let en_good = !looks_like_gibberish(&text_en);
    let ru_good = !looks_like_gibberish(&text_ru);

    let best = if en_good && ru_good {
        if text_en.len() >= text_ru.len() { text_en } else { text_ru }
    } else if en_good {
        text_en
    } else if ru_good {
        text_ru
    } else {
        if text_en.len() >= text_ru.len() { text_en } else { text_ru }
    };

    let result = strip_hallucinations(&best);
    eprintln!("voice: result (fallback) -> '{}'", result);
    Ok(result)
}

/// Один проход распознавания. Возвращает текст. Публичная на уровне крейта —
/// переиспользуется wake-word слушателем (со своей, меньшей моделью).
pub(crate) fn run_whisper_pass(
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

    // Подсказка модели о языке — помогает на коротких фразах и убирает
    // случайные языковые артефакты (грузинский, индейский и т.д.)
    params.set_initial_prompt("Transcribe in Russian or English only.");

    state.full(params, samples).map_err(|e| e.to_string())?;

    let mut text = String::new();
    for segment in state.as_iter() {
        text.push_str(&segment.to_string());
    }

    Ok(text.trim().to_string())
}