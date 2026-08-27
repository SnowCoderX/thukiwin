/*!
 * Фоновое прослушивание wake-word "туки".
 *
 * Работает всё время, пока приложение запущено, кроме моментов, когда:
 * - идёт ручная или авто-командная запись голоса через voice.rs (микрофон
 *   занят другим потоком — см. `suppressed`);
 * - пользователь выключил прослушивание (`enabled`);
 * - ассистент ещё генерирует предыдущий ответ (проверяем
 *   `commands::GenerationState::is_active()`), чтобы повторное "туки" не
 *   перебивало текущий ответ — по договорённости такие срабатывания
 *   игнорируются, а не отменяют текущее.
 *
 * Использует отдельную лёгкую модель Whisper (ggml-small) для быстрого,
 * недорогого спота ключевого слова — отдельно от большой модели, которая
 * транскрибирует сами команды после срабатывания (voice.rs использует свою,
 * уже загруженную большую модель через VoiceState).
 *
 * Детекция самого слова "туки" на каждом вырезанном сегменте гоняется
 * дважды — с принудительным ru и с принудительным en — вместо одного
 * прохода с жёстко зашитым языком. Раньше был только force_lang: Some("ru"),
 * из-за чего английская речь, сказанная сразу после "туки" в одно дыхание
 * (без паузы), "русифицировалась" уже на этапе распознавания wake-word.
 * А текст самой КОМАНДЫ (то, что идёт после "туки") берётся отдельным,
 * уже не форсированным проходом через voice::transcribe_chunk — потому что
 * форсированный language-проход на "чужом" языке иногда не транскрибирует,
 * а натурально переводит фразу на навязанный язык, и русская команда могла
 * улететь в ask() переведённой на английский (и наоборот).
 */

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::voice;

/// Тишина короче, чем у команд после срабатывания — слово "туки" короткое
/// само по себе, не нужно ждать так же долго, как для целой фразы.
const WAKE_SILENCE_SECS: f64 = 0.7;
const WAKE_MIN_CHUNK_SECS: f64 = 0.2;
const WAKE_SILENCE_RMS_THRESHOLD: f32 = 0.005;
const POLL_INTERVAL_MS: u64 = 150;
const PAUSE_CHECK_INTERVAL_MS: u64 = 250;

/// Кандидаты на "туки", как их слышит whisper при принудительном ru —
/// короткие слова whisper часто "слышит" неточно, поэтому сравниваем
/// нечётко (расстояние Левенштейна).
const WAKE_WORD_CANDIDATES_RU: &[&str] = &["туки", "туку", "тука", "тьюки"];
/// Кандидаты на то же слово на слух при принудительном en — то, как оно
/// может транслитерироваться латиницей на слух модели.
const WAKE_WORD_CANDIDATES_EN: &[&str] = &["thuki", "tuki", "tooky", "tookie", "chuki", "2k", "2ki"];
const MAX_EDIT_DISTANCE: usize = 2;

const WAKE_WORD_EVENT: &str = "thuki://wake-word";

/// Общее состояние wake-word слушателя, управляется через app.manage().
pub struct WakeWordState {
    /// Пользовательский переключатель (можно выключить прослушивание совсем).
    pub enabled: Arc<AtomicBool>,
    /// true, пока идёт ручная/авто запись через voice.rs — слушатель отпускает микрофон.
    pub suppressed: Arc<AtomicBool>,
}

impl WakeWordState {
    pub fn new() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            suppressed: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for WakeWordState {
    fn default() -> Self {
        Self::new()
    }
}

#[tauri::command]
pub fn set_wake_word_enabled(enabled: bool, state: tauri::State<'_, WakeWordState>) {
    state.enabled.store(enabled, Ordering::SeqCst);
    eprintln!(
        "wakeword: прослушивание {}",
        if enabled { "включено" } else { "выключено" }
    );
}

#[tauri::command]
pub fn get_wake_word_enabled(state: tauri::State<'_, WakeWordState>) -> bool {
    state.enabled.load(Ordering::SeqCst)
}

/// Запускает фоновый поток прослушивания wake-word. Вызывается один раз при
/// старте приложения (см. `setup()` в lib.rs). Модель грузится лениво (с
/// повтором при неудаче), чтобы не блокировать запуск, если файл модели ещё
/// не скачан — приложение работает нормально, просто без wake-word, пока
/// модель не появится на диске.
pub fn spawn_listener(app: AppHandle, model_path: PathBuf) {
    std::thread::spawn(move || {
        let ctx: Arc<WhisperContext> = loop {
            match WhisperContext::new_with_params(
                model_path.to_str().unwrap_or_default(),
                WhisperContextParameters::default(),
            ) {
                Ok(c) => break Arc::new(c),
                Err(e) => {
                    eprintln!(
                        "wakeword: не удалось загрузить модель ({model_path:?}): {e}. \
                         Повтор через 30с. Скачай ggml-small.bin с \
                         https://huggingface.co/ggerganov/whisper.cpp или задай \
                         переменную окружения WAKE_WORD_MODEL_PATH.",
                    );
                    std::thread::sleep(std::time::Duration::from_secs(30));
                }
            }
        };
        eprintln!("wakeword: модель загружена, слушаем в фоне");

        loop {
            if !should_listen(&app) {
                std::thread::sleep(std::time::Duration::from_millis(PAUSE_CHECK_INTERVAL_MS));
                continue;
            }
            listen_cycle(&app, &ctx);
        }
    });
}

/// true, если сейчас можно занимать микрофон под wake-word прослушивание:
/// включено пользователем, не идёт ручная/авто запись, и ассистент сейчас
/// не генерирует ответ на предыдущую команду.
fn should_listen(app: &AppHandle) -> bool {
    let wake_state = app.state::<WakeWordState>();
    if !wake_state.enabled.load(Ordering::SeqCst) {
        return false;
    }
    if wake_state.suppressed.load(Ordering::SeqCst) {
        return false;
    }
    let gen_state = app.state::<crate::commands::GenerationState>();
    if gen_state.is_active() {
        return false;
    }
    true
}

/// Открывает микрофон и слушает, нарезая речь по паузам, пока не найдёт
/// wake-word или пока `should_listen` не вернёт false. Возвращается (закрыв
/// микрофон), когда нужно уступить устройство.
fn listen_cycle(app: &AppHandle, ctx: &Arc<WhisperContext>) {
    let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<u32>();

    let capture_buffer = buffer.clone();
    let capture_stop = stop_flag.clone();
    let capture_handle = std::thread::spawn(move || {
        voice::run_capture_thread(capture_buffer, ready_tx, capture_stop);
    });

    let sample_rate = match ready_rx.recv() {
        Ok(rate) if rate > 0 => rate,
        _ => {
            stop_flag.store(true, Ordering::SeqCst);
            let _ = capture_handle.join();
            std::thread::sleep(std::time::Duration::from_secs(2));
            return;
        }
    };

    let window_samples = ((sample_rate as f64) * 0.15) as usize;
    let mut voiced_until: usize = 0;
    let mut segment_start: usize = 0;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));

        if !should_listen(app) {
            break;
        }

        let snapshot_len = buffer.lock().map(|b| b.len()).unwrap_or(segment_start);

        if snapshot_len > voiced_until {
            let tail_start = snapshot_len.saturating_sub(window_samples).max(segment_start);
            let rms = buffer
                .lock()
                .map(|b| voice::rms_of(&b[tail_start..snapshot_len]))
                .unwrap_or(0.0);
            if rms > WAKE_SILENCE_RMS_THRESHOLD {
                voiced_until = snapshot_len;
            }
        }

        let silence_secs = (snapshot_len.saturating_sub(voiced_until)) as f64 / sample_rate as f64;
        let chunk_secs = (voiced_until.saturating_sub(segment_start)) as f64 / sample_rate as f64;

        if silence_secs >= WAKE_SILENCE_SECS && chunk_secs >= WAKE_MIN_CHUNK_SECS {
            let end = voiced_until;
            let chunk = buffer
                .lock()
                .map(|b| b[segment_start..end].to_vec())
                .unwrap_or_default();

            // Освобождаем уже обработанную часть буфера, чтобы память не
            // росла неограниченно на протяжении долгой работы приложения.
            if let Ok(mut b) = buffer.lock() {
                b.drain(0..end);
            }
            segment_start = 0;
            voiced_until = 0;

            let resampled = voice::resample_to_16k(&chunk, sample_rate);

            // Шаг 1 — ТОЛЬКО детекция самого слова "туки"/"thuki". Тут
            // форсированные ru/en проходы работают нормально, слово короткое
            // и однозначное, ошибиться сложно.
            let text_ru = voice::run_whisper_pass(ctx, &resampled, Some("ru")).unwrap_or_default();
            let text_en = voice::run_whisper_pass(ctx, &resampled, Some("en")).unwrap_or_default();

            let wake_detected = match_wake_word(&text_ru, WAKE_WORD_CANDIDATES_RU).is_some()
                || match_wake_word(&text_en, WAKE_WORD_CANDIDATES_EN).is_some();

            if wake_detected {
                // Шаг 2 — текст самой КОМАНДЫ берём НЕ из одного из
                // форсированных проходов выше (при чужом force_lang Whisper
                // иногда не транскрибирует, а натурально ПЕРЕВОДИТ фразу на
                // навязанный язык — из-за этого русская команда улетала в
                // ask() переведённой на английский). Вместо этого
                // перераспознаём тот же кусок аудио уже "умной" функцией
                // voice::transcribe_chunk — она делает auto-detect с
                // проверкой на мусор и не путает язык.
                let good_text = voice::transcribe_chunk(ctx, &chunk, sample_rate).unwrap_or_default();
                let remainder = match_wake_word(&good_text, WAKE_WORD_CANDIDATES_RU)
                    .or_else(|| match_wake_word(&good_text, WAKE_WORD_CANDIDATES_EN))
                    .unwrap_or(good_text.clone());

                eprintln!(
                    "wakeword: обнаружено! (детект ru='{}' en='{}'), итог='{}', остаток='{}'",
                    text_ru, text_en, good_text, remainder
                );
                stop_flag.store(true, Ordering::SeqCst);
                let _ = capture_handle.join();
                trigger_activation(app, remainder);
                return;
            }
            // Не совпало ни в одном языке — продолжаем слушать следующий
            // сегмент в этом же потоке.
            continue;
        }

        // Защита от неограниченного роста буфера во время долгой тишины
        // без единого произнесённого слова.
        if segment_start == 0 && voiced_until == 0 && snapshot_len > sample_rate as usize * 10 {
            if let Ok(mut b) = buffer.lock() {
                let keep_from = b.len().saturating_sub(window_samples);
                b.drain(0..keep_from);
            }
        }
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = capture_handle.join();
}

/// Проверяет, есть ли в тексте (нечётко) слово-кандидат из переданного
/// списка, и если да, возвращает остаток фразы после него. Проверяем не
/// только первое слово: whisper на короткой записи иногда добавляет
/// мусорный токен перед словом (щелчок, вдох) или, наоборот, режет
/// короткое "туки" на два токена вроде "ту" + "ки" — поэтому смотрим все
/// позиции и заодно склейку соседних слов.
fn match_wake_word(text: &str, candidates: &[&str]) -> Option<String> {
    let normalized: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }

    for i in 0..words.len() {
        if i + 1 < words.len() {
            let joined = format!("{}{}", words[i], words[i + 1]);
            if is_wake_word_match(&joined, candidates) {
                let remainder = words.get(i + 2..).unwrap_or(&[]).join(" ");
                return Some(remainder);
            }
        }
        if is_wake_word_match(words[i], candidates) {
            let remainder = words.get(i + 1..).unwrap_or(&[]).join(" ");
            return Some(remainder);
        }
    }

    None
}

/// Нечёткое сравнение одного слова со всеми кандидатами из переданного
/// списка (расстояние Левенштейна).
fn is_wake_word_match(word: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| levenshtein(word, candidate) <= MAX_EDIT_DISTANCE)
}

/// Классическое расстояние Левенштейна между двумя строками (по символам).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (la, lb) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; lb + 1]; la + 1];
    for (i, row) in dp.iter_mut().enumerate().take(la + 1) {
        row[0] = i;
    }
    for j in 0..=lb {
        dp[0][j] = j;
    }
    for i in 1..=la {
        for j in 1..=lb {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[la][lb]
}

/// Показывает оверлей и сигнализирует фронту начать авто-командную запись
/// (фронт слушает `thuki://wake-word` и вызывает `start_voice_recording`
/// с `auto_submit: true`).
fn trigger_activation(app: &AppHandle, prefix_text: String) {
    let app_show = app.clone();
    let _ = app.run_on_main_thread(move || {
        crate::show_overlay(&app_show, crate::context::ActivationContext::empty());
    });

    #[derive(Clone, serde::Serialize)]
    struct WakeWordPayload {
        prefix_text: String,
    }
    let _ = app.emit(WAKE_WORD_EVENT, WakeWordPayload { prefix_text });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_identical_strings() {
        assert_eq!(levenshtein("туки", "туки"), 0);
    }

    #[test]
    fn levenshtein_one_edit() {
        assert_eq!(levenshtein("туки", "туку"), 1);
    }

    #[test]
    fn match_wake_word_exact() {
        assert_eq!(
            match_wake_word("туки", WAKE_WORD_CANDIDATES_RU),
            Some(String::new())
        );
    }

    #[test]
    fn match_wake_word_with_command_in_same_breath() {
        assert_eq!(
            match_wake_word("туки переведи слово apple", WAKE_WORD_CANDIDATES_RU),
            Some("переведи слово apple".to_string())
        );
    }

    #[test]
    fn match_wake_word_fuzzy_variants() {
        // whisper иногда "слышит" туки как туку/тука/etc.
        assert!(match_wake_word("туку что там по погоде", WAKE_WORD_CANDIDATES_RU).is_some());
        assert!(match_wake_word("тука привет", WAKE_WORD_CANDIDATES_RU).is_some());
    }

    #[test]
    fn match_wake_word_rejects_unrelated_speech() {
        assert_eq!(match_wake_word("привет как дела", WAKE_WORD_CANDIDATES_RU), None);
    }

    #[test]
    fn match_wake_word_ignores_leading_noise_token() {
        // whisper иногда добавляет мусорный токен перед реальным словом
        // (щелчок, вдох) — туки может оказаться не первым словом.
        assert_eq!(
            match_wake_word("э туки как дела", WAKE_WORD_CANDIDATES_RU),
            Some("как дела".to_string())
        );
    }

    #[test]
    fn match_wake_word_handles_split_tokens() {
        // короткое слово иногда режется whisper-ом на два токена подряд.
        assert_eq!(
            match_wake_word("ту ки привет", WAKE_WORD_CANDIDATES_RU),
            Some("привет".to_string())
        );
    }

    #[test]
    fn match_wake_word_rejects_empty() {
        assert_eq!(match_wake_word("", WAKE_WORD_CANDIDATES_RU), None);
    }

    #[test]
    fn match_wake_word_english_exact() {
        assert_eq!(
            match_wake_word("thuki", WAKE_WORD_CANDIDATES_EN),
            Some(String::new())
        );
    }

    #[test]
    fn match_wake_word_english_with_command() {
        assert_eq!(
            match_wake_word("thuki what is the weather today", WAKE_WORD_CANDIDATES_EN),
            Some("what is the weather today".to_string())
        );
    }

    #[test]
    fn match_wake_word_english_fuzzy_variant() {
        // whisper иногда "слышит" thuki как tuki/tooky на слух.
        assert!(match_wake_word("tuki translate this word", WAKE_WORD_CANDIDATES_EN).is_some());
    }

    #[test]
    fn wake_word_state_defaults_enabled_and_not_suppressed() {
        let state = WakeWordState::new();
        assert!(!state.enabled.load(Ordering::SeqCst));
        assert!(!state.suppressed.load(Ordering::SeqCst));
    }
}
