use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};

static AUDIO_PLAYING: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn get_flag() -> &'static Arc<AtomicBool> {
    AUDIO_PLAYING.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

pub fn start() {
    std::thread::spawn(|| {
        unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); }

        let enumerator: IMMDeviceEnumerator = unsafe {
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(_) => return,
            }
        };

        let device = unsafe {
            match enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
                Ok(d) => d,
                Err(_) => return,
            }
        };

        let meter: IAudioMeterInformation = unsafe {
            match device.Activate(CLSCTX_ALL, None) {
                Ok(m) => m,
                Err(_) => return,
            }
        };

        loop {
            let peak = unsafe { meter.GetPeakValue().unwrap_or(0.0) };
            let playing = peak > 0.001;
            get_flag().store(playing, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(200));
        }
    });
}

#[tauri::command]
pub fn is_system_audio_playing() -> bool {
    get_flag().load(Ordering::Relaxed)
}