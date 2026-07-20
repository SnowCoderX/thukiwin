// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Sets per-monitor-v2 DPI awareness for the whole process, before any
/// window is created.
///
/// This must happen here — as the very first thing in `main()`, before
/// `thuki_agent_lib::run()` — and not inside `run()` itself or via a
/// per-thread `SetThreadDpiAwarenessContext` call made later. By the time
/// Tauri creates its first window (which happens inside `run()`), Windows
/// has already latched the process's DPI awareness at its default
/// (DPI-virtualized/system-DPI-unaware), and nothing afterwards — thread- or
/// process-level — can override that first decision.
///
/// Without this, `GetSystemMetrics`/`GetDC`/`BitBlt` across the whole process
/// see a virtualized (scaled-down) desktop instead of real pixels. That's
/// exactly why captured screenshots on a multi-monitor HiDPI setup came out
/// uniformly downscaled (e.g. 1920x461 instead of the real ~6000x1440) even
/// though the aspect ratio — and therefore both monitors' relative layout —
/// was preserved: the whole virtual desktop was measured and captured
/// through the same virtualization scale, so it was internally consistent,
/// just consistently wrong.
#[cfg(target_os = "windows")]
fn set_process_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    // SAFETY: this only sets a process-global Win32 flag. It must run before
    // any window (including Tauri's) is created, which is guaranteed by
    // calling it first thing in `main()`, before `thuki_agent_lib::run()`.
    let result = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if let Err(e) = result {
        eprintln!("main: не удалось установить DPI awareness процесса: {e}");
    }
}

/// The main entry point for the desktop application.
///
/// This function calls into the common core library to start the application.
fn main() {
    #[cfg(target_os = "windows")]
    set_process_dpi_awareness();

    thuki_agent_lib::run()
}