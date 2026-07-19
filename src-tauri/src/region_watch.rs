/*!
 * Слежение за областью экрана (Region Watch).
 *
 * Пользователь один раз выделяет прямоугольник на экране (может охватывать
 * оба монитора — координаты глобальные, виртуальный рабочий стол). Пока
 * режим включён, фоновый поток раз в заданный интервал делает скриншот
 * именно этой области, сравнивает с предыдущим кадром через перцептивный
 * хэш (устойчив к шуму сжатия/лёгкому дрожанию, чувствителен к реальной
 * смене содержимого), и если картинка действительно изменилась — сохраняет
 * PNG и шлёт событие с путём к файлу и заданным промптом.
 *
 * Фронт по этому событию сам вызывает обычный `ask()` — так же, как уже
 * сделано для wake-word (thuki://wake-word): единый путь через обычный чат,
 * без отдельного "тихого" механизма на стороне Rust. Смена самой области —
 * только вручную через `start_region_selection` (см. патч для lib.rs/main.tsx),
 * никогда не триггерится автоматически.
 */

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};

/// Прямоугольник в глобальных экранных координатах — может пересекать
/// границы нескольких мониторов (это просто x/y/width/height виртуального
/// рабочего стола, не координаты одного конкретного монитора).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WatchRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Конфигурация слежения — то, что пользователь задаёт в попапе.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RegionWatchConfig {
    pub rect: Option<WatchRect>,
    pub prompt: String,
    /// Учитывать ли системный промпт активного профиля вместе с `prompt`.
    pub use_profile: bool,
    /// Интервал между проверками экрана, в миллисекундах. Ниже
    /// `MIN_INTERVAL_MS` не даём поставить — защита от случайного 0/1мс.
    pub interval_ms: u64,
}

impl Default for RegionWatchConfig {
    fn default() -> Self {
        Self {
            rect: None,
            prompt: String::new(),
            use_profile: true,
            interval_ms: 1500,
        }
    }
}

const REGION_WATCH_FRAME_EVENT: &str = "thuki://region-watch-frame";
const MIN_INTERVAL_MS: u64 = 300;
/// Пока не включено или область не выбрана, поток просто спит и
/// перепроверяет флаги с этим интервалом — не имеет смысла делать его
/// настраиваемым, это не про частоту проверки экрана.
const IDLE_POLL_MS: u64 = 500;

pub struct RegionWatchState {
    config: Mutex<RegionWatchConfig>,
    enabled: Arc<AtomicBool>,
    /// Увеличивается на каждое изменение конфига/rect — не строго
    /// необходим для текущей логики (поток и так перечитывает config
    /// каждую итерацию), но пригодится, если решим добавить
    /// синхронизацию паузы во время выбора региона (см. TODO в lib.rs патче).
    generation: Arc<AtomicU64>,
}

impl RegionWatchState {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(RegionWatchConfig::default()),
            enabled: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Default for RegionWatchState {
    fn default() -> Self {
        Self::new()
    }
}

#[tauri::command]
pub fn get_region_watch_config(state: tauri::State<'_, RegionWatchState>) -> RegionWatchConfig {
    state.config.lock().unwrap().clone()
}

/// Сохраняет конфиг целиком (rect/prompt/use_profile/interval_ms). Вызывается
/// и из попапа настроек, и из `start_region_selection` (обновляет только rect).
#[tauri::command]
pub fn set_region_watch_config(config: RegionWatchConfig, state: tauri::State<'_, RegionWatchState>) {
    let mut guard = state.config.lock().unwrap();
    *guard = config;
    state.generation.fetch_add(1, Ordering::SeqCst);
}

/// Обновляет только `rect` в конфиге, не трогая prompt/use_profile/interval_ms.
/// Используется `finish_region_selection` (lib.rs) после того, как пользователь
/// мышкой выделил область — так попап настроек не нужно открывать заново.
#[tauri::command]
pub fn set_region_watch_rect(rect: Option<WatchRect>, state: tauri::State<'_, RegionWatchState>) {
    let mut guard = state.config.lock().unwrap();
    guard.rect = rect;
    drop(guard);
    state.generation.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command]
pub fn set_region_watch_enabled(enabled: bool, state: tauri::State<'_, RegionWatchState>) {
    state.enabled.store(enabled, Ordering::SeqCst);
    state.generation.fetch_add(1, Ordering::SeqCst);
    eprintln!(
        "region_watch: слежение {}",
        if enabled { "включено" } else { "выключено" }
    );
}

#[tauri::command]
pub fn get_region_watch_enabled(state: tauri::State<'_, RegionWatchState>) -> bool {
    state.enabled.load(Ordering::SeqCst)
}

/// Запускает фоновый поток слежения. Вызывается один раз при старте
/// приложения (см. `setup()` в lib.rs) — сам поток бездействует
/// (спит и перепроверяет флаги), пока `enabled` не станет true И `rect`
/// не будет задан.
pub fn spawn_watch_loop(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_hash: Option<u64> = None;
        let mut last_rect: Option<WatchRect> = None;

        loop {
            let (enabled, config) = {
                let state = app.state::<RegionWatchState>();
                // Не возвращаем кортеж напрямую как хвостовое выражение блока —
                // временный MutexGuard из `.lock()` в этом случае дропается
                // ПОСЛЕ локальных переменных блока (включая `state`), а не до,
                // из-за правил Rust для временных значений в хвостовом
                // выражении. `state` — это заимствование, так что получаем
                // E0597 ("state dropped here while still borrowed"), хотя
                // guard к этому моменту уже не нужен (клонирование прошло).
                // Промежуточная переменная форсирует дроп guard'а в конце
                // этого let-выражения, до дропа `state` в конце блока.
                let snapshot = (
                    state.enabled.load(Ordering::SeqCst),
                    state.config.lock().unwrap().clone(),
                );
                snapshot
            };

            if !enabled || config.rect.is_none() {
                last_hash = None;
                last_rect = None;
                std::thread::sleep(std::time::Duration::from_millis(IDLE_POLL_MS));
                continue;
            }

            let rect = config.rect.expect("checked is_none above");

            // Смена самой области сбрасывает хэш — иначе первый кадр после
            // ручного редактирования rect может ошибочно посчитаться
            // "таким же, как предыдущий" (сравнивали бы с кадром старой области).
            if last_rect != Some(rect) {
                last_hash = None;
                last_rect = Some(rect);
            }

            let interval = config.interval_ms.max(MIN_INTERVAL_MS);

            match capture_region(&rect) {
                Ok((png_bytes, hash)) => {
                    if last_hash != Some(hash) {
                        last_hash = Some(hash);
                        if let Err(e) = emit_frame(&app, &png_bytes, &config) {
                            eprintln!("region_watch: не удалось обработать кадр: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("region_watch: ошибка захвата области: {e}");
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(interval));
        }
    });
}

/// Захватывает пиксели заданного прямоугольника, кодирует в PNG и считает
/// перцептивный хэш по уменьшенной версии кадра.
fn capture_region(rect: &WatchRect) -> Result<(Vec<u8>, u64), String> {
    let (width, height, rgba) = platform_capture(rect.x, rect.y, rect.width, rect.height)?;

    let buf = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba)
        .ok_or_else(|| "не удалось собрать изображение из захваченных пикселей".to_string())?;
    let dynamic = image::DynamicImage::ImageRgba8(buf);

    let hash = perceptual_hash(&dynamic);

    let mut png = Vec::new();
    dynamic
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("не удалось закодировать PNG: {e}"))?;

    Ok((png, hash))
}

#[cfg(target_os = "windows")]
fn platform_capture(x: i32, y: i32, width: u32, height: u32) -> Result<(u32, u32, Vec<u8>), String> {
    crate::windows_screenshot::capture_monitor_pixels(x, y, width, height)
}

#[cfg(target_os = "macos")]
fn platform_capture(
    _x: i32,
    _y: i32,
    _width: u32,
    _height: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    // На macOS то же самое можно сделать через CGWindowListCreateImage с
    // произвольным CGRect (не обязательно весь экран) — по аналогии с тем,
    // что уже есть в screenshot.rs для capture_full_screen_raw. Не стал
    // добавлять сейчас, раз собираете thukiwin — дам по аналогии отдельно,
    // если понадобится.
    Err("слежение за областью пока реализовано только для Windows".to_string())
}

/// Грубый перцептивный хэш (упрощённый average-hash): уменьшаем кадр до
/// 8x8 в градациях серого, сравниваем каждый пиксель со средней яркостью,
/// пакуем результат в 64-битное число. Устойчив к шуму сжатия и небольшим
/// колебаниям яркости/сглаживания шрифта, но чувствителен к реальной смене
/// текста/субтитров/содержимого — то, что нужно для дедупликации кадров.
fn perceptual_hash(image: &image::DynamicImage) -> u64 {
    const SIZE: u32 = 8;
    let small = image
        .resize_exact(SIZE, SIZE, image::imageops::FilterType::Triangle)
        .to_luma8();

    let pixels: Vec<u8> = small.pixels().map(|p| p.0[0]).collect();
    let avg = pixels.iter().map(|&p| p as u32).sum::<u32>() / pixels.len().max(1) as u32;

    let mut hash: u64 = 0;
    for (i, &p) in pixels.iter().enumerate() {
        if (p as u32) >= avg {
            hash |= 1 << i;
        }
    }
    hash
}

/// Сохраняет кадр на диск и шлёт событие фронту. Фронт сам решает, вызывать
/// ли `ask()` с учётом профиля (`use_profile`) или без — с фиксированным,
/// не меняющимся текстом `prompt` в качестве `promptOverride`.
fn emit_frame(app: &AppHandle, png_bytes: &[u8], config: &RegionWatchConfig) -> Result<(), String> {
    let base_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("не удалось получить app_data_dir: {e}"))?;
    let path = crate::images::save_image(&base_dir, png_bytes)?;

    #[derive(Clone, serde::Serialize)]
    struct RegionWatchFramePayload {
        path: String,
        prompt: String,
        use_profile: bool,
    }

    let _ = app.emit(
        REGION_WATCH_FRAME_EVENT,
        RegionWatchFramePayload {
            path,
            prompt: config.prompt.clone(),
            use_profile: config.use_profile,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_rect_and_is_conservative() {
        let config = RegionWatchConfig::default();
        assert!(config.rect.is_none());
        assert!(config.use_profile);
        assert_eq!(config.interval_ms, 1500);
    }

    #[test]
    fn watch_rect_equality() {
        let a = WatchRect { x: 0, y: 0, width: 100, height: 100 };
        let b = WatchRect { x: 0, y: 0, width: 100, height: 100 };
        let c = WatchRect { x: 1, y: 0, width: 100, height: 100 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn perceptual_hash_identical_images_match() {
        let img1 = image::DynamicImage::new_rgba8(32, 32);
        let img2 = image::DynamicImage::new_rgba8(32, 32);
        assert_eq!(perceptual_hash(&img1), perceptual_hash(&img2));
    }

    #[test]
    fn perceptual_hash_different_images_differ() {
        let mut buf1 = image::RgbaImage::new(32, 32);
        for p in buf1.pixels_mut() {
            *p = image::Rgba([0, 0, 0, 255]);
        }
        let mut buf2 = image::RgbaImage::new(32, 32);
        for (i, p) in buf2.pixels_mut().enumerate() {
            *p = if i % 2 == 0 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            };
        }
        let img1 = image::DynamicImage::ImageRgba8(buf1);
        let img2 = image::DynamicImage::ImageRgba8(buf2);
        assert_ne!(perceptual_hash(&img1), perceptual_hash(&img2));
    }

    #[test]
    fn state_defaults_disabled() {
        let state = RegionWatchState::new();
        assert!(!state.enabled.load(Ordering::SeqCst));
    }
}