//! Windows screenshot capture using GDI BitBlt.

use tauri::Manager;

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetBitmapBits,
    GetDC, ReleaseDC, SelectObject, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// Captures the given desktop region using GDI BitBlt and returns raw RGBA pixels.
#[cfg(target_os = "windows")]
pub fn capture_monitor_pixels(
    origin_x: i32,
    origin_y: i32,
    width: u32,
    height: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        if width == 0 || height == 0 {
            return Err("Failed to get monitor dimensions".to_string());
        }

        let screen_dc = GetDC(None);

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        let _old_bitmap = SelectObject(mem_dc, bitmap.into());

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

        // Deselect bitmap from DC before reading — GetBitmapBits must not
        // be called on a bitmap currently selected into a device context.
        SelectObject(mem_dc, _old_bitmap);

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
        let _ = ReleaseDC(None, screen_dc);

        if bits_copied == 0 {
            return Err("GetBitmapBits returned 0 bytes".to_string());
        }

        // Convert BGRA to RGBA in-place.
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        Ok((width, height, pixels))
    }
}

/// Fallback capture for the primary desktop area when monitor lookup fails.
#[cfg(target_os = "windows")]
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

/// Tauri command: silently captures the full screen and returns the absolute
/// file path of the saved image.
#[cfg(target_os = "windows")]
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
                capture_monitor_pixels(position.x, position.y, size.width, size.height)?
            }
            None => capture_primary_screen_pixels()?,
        };

        let buf =
            image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba_bytes)
                .ok_or_else(|| "Failed to create image buffer from captured pixels.".to_string())?;
        let dynamic = image::DynamicImage::ImageRgba8(buf);

        let mut png: Vec<u8> = Vec::new();
        dynamic
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode screen capture as PNG: {e}"))?;

        crate::images::save_image(&base_dir, &png)
    })
    .await
    .map_err(|e| format!("image encoding task failed: {e}"))?;

    result
}

/// Captures a user-selected screen region. On Windows, returns None for now.
#[cfg(target_os = "windows")]
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn capture_screenshot_command(
    _app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    Ok(None)
}
