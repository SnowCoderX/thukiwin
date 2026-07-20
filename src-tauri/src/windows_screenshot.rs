//! Windows screenshot capture using GDI BitBlt.

#![allow(dead_code)]

use tauri::Manager;

use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors,
    GetBitmapBits, GetDC, GetMonitorInfoW, HDC, HMONITOR, MONITORENUMPROC, MONITORINFO, ReleaseDC,
    SelectObject, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetDesktopWindow, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

#[derive(Debug, Clone, Copy, Default)]
struct Bounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
    found: bool,
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _lprc_clip: *mut RECT,
    lparam: LPARAM,
) -> i32 {
    let mut info = MONITORINFO::default();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;

    if GetMonitorInfoW(hmonitor, &mut info).as_bool() {
        let bounds = &mut *(lparam.0 as *mut Bounds);
        let r = info.rcMonitor;

        if !bounds.found {
            bounds.min_x = r.left;
            bounds.min_y = r.top;
            bounds.max_x = r.right;
            bounds.max_y = r.bottom;
            bounds.found = true;
        } else {
            if r.left < bounds.min_x { bounds.min_x = r.left; }
            if r.top < bounds.min_y { bounds.min_y = r.top; }
            if r.right > bounds.max_x { bounds.max_x = r.right; }
            if r.bottom > bounds.max_y { bounds.max_y = r.bottom; }
        }
    }

    1
}

/// Computes the bounding box of all monitors in physical pixels.
/// Kept for future use, but NOT used in capture_desktop_for_selection.
fn get_virtual_desktop_rect() -> Result<(i32, i32, u32, u32), String> {
    let mut bounds = Bounds::default();

    unsafe {
        let proc: MONITORENUMPROC = Some(std::mem::transmute(
            monitor_enum_proc as unsafe extern "system" fn(HMONITOR, HDC, *mut RECT, LPARAM) -> i32
        ));
        let result = EnumDisplayMonitors(
            None,
            None,
            proc,
            LPARAM(&mut bounds as *mut _ as isize),
        );
        if !result.as_bool() {
            return Err("EnumDisplayMonitors failed".to_string());
        }
    }

    if !bounds.found {
        return Err("No monitors found".to_string());
    }

    let width = (bounds.max_x - bounds.min_x) as u32;
    let height = (bounds.max_y - bounds.min_y) as u32;

    eprintln!(
        "[screenshot] virtual desktop rect: x={}, y={}, w={}, h={}",
        bounds.min_x, bounds.min_y, width, height
    );

    Ok((bounds.min_x, bounds.min_y, width, height))
}

/// Captures a specific region using GDI BitBlt.
/// GetDC(GetDesktopWindow) gives a DC for the entire virtual desktop,
/// so this works for any monitor as long as origin/size are correct.
pub fn capture_monitor_pixels(
    origin_x: i32,
    origin_y: i32,
    width: u32,
    height: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        if width == 0 || height == 0 {
            return Err("Width or height is zero".to_string());
        }

        eprintln!(
            "[screenshot] capture region: origin=({},{}) size={}x{}",
            origin_x, origin_y, width, height
        );

        let hwnd = GetDesktopWindow();
        let screen_dc = GetDC(Some(hwnd));
        if screen_dc.is_invalid() {
            return Err("GetDC(GetDesktopWindow) returned invalid HDC".to_string());
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.is_invalid() {
            let _ = ReleaseDC(Some(hwnd), screen_dc);
            return Err("CreateCompatibleDC failed".to_string());
        }

        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        if bitmap.is_invalid() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(Some(hwnd), screen_dc);
            return Err("CreateCompatibleBitmap failed".to_string());
        }

        let old_bitmap = SelectObject(mem_dc, bitmap.into());
        if old_bitmap.is_invalid() {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(Some(hwnd), screen_dc);
            return Err("SelectObject failed".to_string());
        }

        BitBlt(
            mem_dc,
            0,
            0,
            width as i32,
            height as i32,
            Some(screen_dc),
            origin_x,
            origin_y,
            SRCCOPY,
        )
        .map_err(|e| format!("BitBlt failed: {e}"))?;

        SelectObject(mem_dc, old_bitmap);

        let row_size = width * 4;
        let pixel_size = (row_size * height) as usize;
        let mut pixels: Vec<u8> = vec![0u8; pixel_size];

        let bits_copied = GetBitmapBits(
            bitmap,
            pixel_size as i32,
            pixels.as_mut_ptr() as *mut core::ffi::c_void,
        );

        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(Some(hwnd), screen_dc);

        if bits_copied == 0 {
            return Err("GetBitmapBits returned 0 bytes".to_string());
        }

        eprintln!(
            "[screenshot] copied {} bytes, expected {}",
            bits_copied, pixel_size
        );

        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        Ok((width, height, pixels))
    }
}

/// Fallback: captures the primary monitor only.
pub fn capture_primary_screen_pixels() -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 {
            return Err("Failed to get primary screen dimensions".to_string());
        }
        capture_monitor_pixels(0, 0, width as u32, height as u32)
    }
}

/// Captures the entire virtual desktop (all monitors). Kept for future.
pub fn capture_virtual_desktop_pixels() -> Result<(u32, u32, Vec<u8>), String> {
    let (x, y, width, height) = get_virtual_desktop_rect()?;
    capture_monitor_pixels(x, y, width, height)
}

/// Returns virtual desktop bounds. Kept for future.
pub fn get_virtual_desktop_size() -> Result<(i32, i32, i32, i32), String> {
    let (x, y, w, h) = get_virtual_desktop_rect()?;
    Ok((x, y, w as i32, h as i32))
}

/// Saves raw RGBA pixels as PNG.
pub fn save_rgba_as_png(
    width: u32,
    height: u32,
    rgba_bytes: Vec<u8>,
    base_dir: &std::path::Path,
) -> Result<String, String> {
    let buf = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba_bytes)
        .ok_or_else(|| "Failed to create image buffer from captured pixels.".to_string())?;
    let dynamic = image::DynamicImage::ImageRgba8(buf);

    let mut png: Vec<u8> = Vec::new();
    dynamic
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode screen capture as PNG: {e}"))?;

    crate::images::save_image(base_dir, &png)
}

/// Command: captures the screen where the main window is currently located.
/// Used for attaching screenshot to the chat.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn capture_full_screen_command(app_handle: tauri::AppHandle) -> Result<String, String> {
    let base_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;

    let monitor = app_handle
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?
        .current_monitor()
        .map_err(|e| format!("failed to get current monitor: {e}"))?;

    let result = tokio::task::spawn_blocking(move || {
        let (width, height, rgba_bytes) = match monitor {
            Some(monitor) => {
                let position = monitor.position();
                let size = monitor.size();
                eprintln!(
                    "[screenshot] full_screen: pos=({},{}), size={}x{}",
                    position.x, position.y, size.width, size.height
                );
                capture_monitor_pixels(position.x, position.y, size.width, size.height)?
            }
            None => capture_primary_screen_pixels()?,
        };

        save_rgba_as_png(width, height, rgba_bytes, &base_dir)
    })
    .await
    .map_err(|e| format!("image encoding task failed: {e}"))?;

    result
}

/// Command: captures ONLY the monitor where the main window is located.
/// This is the region-selection screenshot — single monitor, reliable, no multi-monitor bullshit.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn capture_desktop_for_selection(app_handle: tauri::AppHandle) -> Result<String, String> {
    let base_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;

    let monitor = app_handle
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?
        .current_monitor()
        .map_err(|e| format!("failed to get current monitor: {e}"))?;

    let result = tokio::task::spawn_blocking(move || {
        let (width, height, rgba_bytes) = match monitor {
            Some(monitor) => {
                let position = monitor.position();
                let size = monitor.size();
                eprintln!(
                    "[screenshot] selection: pos=({},{}), size={}x{}",
                    position.x, position.y, size.width, size.height
                );
                capture_monitor_pixels(position.x, position.y, size.width, size.height)?
            }
            None => {
                eprintln!("[screenshot] selection: falling back to primary screen");
                capture_primary_screen_pixels()?
            }
        };

        eprintln!("[screenshot] selection saved: {}x{}", width, height);
        save_rgba_as_png(width, height, rgba_bytes, &base_dir)
    })
    .await
    .map_err(|e| format!("image encoding task failed: {e}"))?;

    result
}

/// Command: captures an explicit region given physical coordinates supplied
/// by the frontend (from `availableMonitors()`), for the screenshot button's
/// right-click "choose monitor" menu.
///
/// Deliberately takes coordinates instead of a monitor index: Tauri's JS
/// `availableMonitors()` and Rust's `available_monitors()` are not guaranteed
/// to enumerate monitors in the same order, so an index picked in JS could
/// silently map to the wrong Rust monitor. Physical x/y/width/height are
/// unambiguous, and the frontend already has them from the same monitor
/// list it used to build the menu.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn capture_screen_region_command(
    app_handle: tauri::AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let base_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;

    let result = tokio::task::spawn_blocking(move || {
        eprintln!("[screenshot] region: origin=({x},{y}) size={width}x{height}");
        let (w, h, rgba_bytes) = capture_monitor_pixels(x, y, width, height)?;
        save_rgba_as_png(w, h, rgba_bytes, &base_dir)
    })
    .await
    .map_err(|e| format!("image encoding task failed: {e}"))?;

    result
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn capture_screenshot_command(
    _app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    Ok(None)
}