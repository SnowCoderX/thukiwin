use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionWatchConfig {
    pub rect: Option<Rect>,
    pub prompt: String,
    pub use_profile: bool,
    pub interval_ms: u64,
}

impl Default for RegionWatchConfig {
    fn default() -> Self {
        Self {
            rect: None,
            prompt: String::new(),
            use_profile: false,
            interval_ms: 3000,
        }
    }
}

#[derive(Clone, serde::Serialize)]
pub struct RegionWatchFramePayload {
    pub path: String,
    pub prompt: String,
    pub use_profile: bool,
}

pub struct RegionWatchState {
    pub config: Mutex<RegionWatchConfig>,
    pub enabled: AtomicBool,
    pub last_hash: Mutex<Option<u64>>,
}

impl RegionWatchState {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(RegionWatchConfig::default()),
            enabled: AtomicBool::new(false),
            last_hash: Mutex::new(None),
        }
    }
}

fn hash_pixels(pixels: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    pixels.hash(&mut hasher);
    hasher.finish()
}

pub fn spawn_watch_loop(app_handle: AppHandle) {
    thread::spawn(move || {
        loop {
            // 1. Читаем конфиг
            let (enabled, interval_ms, rect_opt, prompt, use_profile) = {
                let state = app_handle.state::<RegionWatchState>();
                let config = state.config.lock().unwrap();
                (
                    state.enabled.load(Ordering::SeqCst),
                    config.interval_ms,
                    config.rect.clone(),
                    config.prompt.clone(),
                    config.use_profile,
                )
            };

            if !enabled || rect_opt.is_none() {
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            let rect = match rect_opt {
                Some(r) if r.is_valid() => r,
                _ => {
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };

            // 2. Если интервал 0, спим 500мс, чтобы не грузить CPU в ноль
            if interval_ms == 0 {
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            // 3. СТРОГО ЖДЕМ УКАЗАННЫЙ ИНТЕРВАЛ.
            // Никаких захватов и проверок, пока время не вышло!
            thread::sleep(Duration::from_millis(interval_ms));

            // 4. Время вышло — делаем захват
            match crate::windows_screenshot::capture_monitor_pixels(
                rect.x, rect.y, rect.width as u32, rect.height as u32,
            ) {
                Ok((width, height, rgba)) => {
                    let pixel_hash = hash_pixels(&rgba);

                    let should_emit = {
                        let state = app_handle.state::<RegionWatchState>();
                        let mut last = state.last_hash.lock().unwrap();
                        let changed = last.map_or(true, |h| h != pixel_hash);
                        if changed {
                            *last = Some(pixel_hash);
                        }
                        changed
                    };

                    if should_emit {
                        let base_dir = match app_handle.path().app_data_dir() {
                            Ok(d) => d,
                            Err(e) => {
                                eprintln!("[region_watch] app_data_dir error: {}", e);
                                continue;
                            }
                        };

                        let watch_dir = base_dir.join("region_watch");
                        let _ = std::fs::create_dir_all(&watch_dir);

                        let timestamp = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;

                        let filename = format!("capture_{}.png", timestamp);
                        let path = watch_dir.join(&filename);

                        match save_rgba_to_file(width, height, rgba, &path) {
                            Ok(()) => {
                                let _ = app_handle.emit("thuki://region-watch-frame", RegionWatchFramePayload {
                                    path: path.to_string_lossy().to_string(),
                                    prompt: prompt.clone(),
                                    use_profile,
                                });
                            }
                            Err(e) => eprintln!("[region_watch] save error: {}", e),
                        }
                    }
                }
                Err(e) => eprintln!("[region_watch] capture error: {}", e),
            }
        }
    });
}

fn save_rgba_to_file(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    path: &std::path::Path,
) -> Result<(), String> {
    let buf = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba)
        .ok_or_else(|| "Failed to create image buffer".to_string())?;
    let file = std::fs::File::create(path)
        .map_err(|e| format!("Failed to create file: {e}"))?;
    image::DynamicImage::ImageRgba8(buf)
        .write_to(&mut std::io::BufWriter::new(file), image::ImageFormat::Png)
        .map_err(|e| format!("Failed to write PNG: {e}"))?;
    Ok(())
}

// ─── Commands ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_region_watch_config(state: State<RegionWatchState>) -> Result<RegionWatchConfig, String> {
    Ok(state.config.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub fn set_region_watch_config(
    state: State<RegionWatchState>,
    config: RegionWatchConfig,
) -> Result<(), String> {
    *state.config.lock().map_err(|e| e.to_string())? = config;
    Ok(())
}

#[tauri::command]
pub fn set_region_watch_rect(
    state: State<RegionWatchState>,
    rect: Option<Rect>,
) -> Result<(), String> {
    state.config.lock().map_err(|e| e.to_string())?.rect = rect;
    Ok(())
}

#[tauri::command]
pub fn set_region_watch_enabled(
    state: State<RegionWatchState>,
    enabled: bool,
) -> Result<(), String> {
    state.enabled.store(enabled, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn get_region_watch_enabled(state: State<RegionWatchState>) -> Result<bool, String> {
    Ok(state.enabled.load(Ordering::SeqCst))
}