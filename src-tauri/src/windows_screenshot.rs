//! Windows screenshot capture using GDI BitBlt.

use tauri::Manager;

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDCW, DeleteDC, DeleteObject,
    GetBitmapBits, SelectObject, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
use windows::core::PCWSTR;

/// Captures the full screen using GDI BitBlt and returns raw RGBA pixel bytes.
#[cfg(target_os = "windows")]
pub fn capture_full_screen_pixels() -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);

        if screen_width <= 0 || screen_height <= 0 {
            return Err("Failed to get screen dimensions".to_string());
        }

        let width = screen_width as u32;
        let height = screen_height as u32;

        let screen_dc = CreateDCW(
            PCWSTR::null(),
            windows::core::w!("DISPLAY"),
            PCWSTR::null(),
            None,
        );

        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, screen_width, screen_height);
        let _old_bitmap = SelectObject(mem_dc, bitmap);

        let _ = BitBlt(
            mem_dc, 0, 0, screen_width, screen_height,
            screen_dc, 0, 0,
            SRCCOPY,
        );

        let row_size = width * 4;
        let pixel_size = (row_size * height) as usize;
        let mut pixels: Vec<u8> = vec![0u8; pixel_size];

        let bits_copied = GetBitmapBits(
            bitmap,
            pixel_size as i32,
            pixels.as_mut_ptr() as *mut core::ffi::c_void,
        );

        SelectObject(mem_dc, _old_bitmap);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);
        let _ = DeleteDC(screen_dc);

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

/// Tauri command: silently captures the full screen and returns the absolute
/// file path of the saved image.
#[cfg(target_os = "windows")]
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn capture_full_screen_command(app_handle: tauri::AppHandle) -> Result<String, String> {
    let base_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;

    let result = tokio::task::spawn_blocking(move || {
        let (width, height, rgba_bytes) = capture_full_screen_pixels()?;

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