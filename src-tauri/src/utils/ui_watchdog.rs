#[cfg(not(target_os = "windows"))]
pub const fn watch(_window: &tauri::WebviewWindow) {}

#[cfg(not(target_os = "windows"))]
pub const fn responds() -> bool {
    true
}

#[cfg(target_os = "windows")]
pub use windows_watchdog::{responds, watch};

#[cfg(target_os = "windows")]
mod windows_watchdog {
    use std::{
        os::windows::process::CommandExt as _,
        process::Command,
        sync::atomic::{AtomicBool, AtomicIsize, Ordering},
        time::{Duration, Instant},
    };

    use clash_verge_logging::{Type, logging};

    const POLL: Duration = Duration::from_secs(2);
    const ANSWER: Duration = Duration::from_millis(1500);
    const REMIND: Duration = Duration::from_secs(30);
    const RESTART_AFTER: Duration = Duration::from_secs(20);
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
    static STARTED: AtomicBool = AtomicBool::new(false);

    pub fn watch(window: &tauri::WebviewWindow) {
        match window.hwnd() {
            Ok(hwnd) => {
                MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
                start();
            }
            Err(e) => logging!(warn, Type::Window, "Сторож интерфейса не получил окно: {}", e),
        }
    }

    pub fn responds() -> bool {
        let hwnd = MAIN_HWND.load(Ordering::Relaxed);
        hwnd == 0 || window_answers(hwnd)
    }

    fn window_answers(hwnd: isize) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SMTO_ABORTIFHUNG, SMTO_BLOCK, SendMessageTimeoutW, WM_NULL};

        let mut answer: usize = 0;
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd as *mut core::ffi::c_void,
                WM_NULL,
                0,
                0,
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                ANSWER.as_millis() as u32,
                &raw mut answer,
            )
        };

        sent != 0
    }

    fn start() {
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }

        match std::thread::Builder::new().name("ui-watchdog".into()).spawn(watch_loop) {
            Ok(_) => logging!(info, Type::Window, "Сторож интерфейса запущен"),
            Err(e) => {
                STARTED.store(false, Ordering::SeqCst);
                logging!(warn, Type::Window, "Не удалось запустить сторож интерфейса: {}", e);
            }
        }
    }

    fn restart_self() {
        let Ok(exe) = std::env::current_exe() else {
            logging!(error, Type::Window, "Перезапуск невозможен: путь приложения неизвестен");
            return;
        };
        let pid = std::process::id();
        let script = format!(
            "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; Start-Process -FilePath '{}'",
            exe.to_string_lossy().replace('\'', "''")
        );
        let spawned = Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        match spawned {
            Ok(_) => {
                logging!(
                    error,
                    Type::Window,
                    "Интерфейс не ответил за {} с, приложение перезапускается",
                    RESTART_AFTER.as_secs()
                );
                log::logger().flush();
                std::thread::sleep(Duration::from_millis(200));
                std::process::exit(0);
            }
            Err(e) => logging!(error, Type::Window, "Не удалось запланировать перезапуск: {}", e),
        }
    }

    fn watch_loop() {
        let mut hung_since: Option<Instant> = None;
        let mut reminded_at = Instant::now();

        loop {
            std::thread::sleep(POLL);

            let hwnd = MAIN_HWND.load(Ordering::Relaxed);
            if hwnd == 0 {
                continue;
            }

            if window_answers(hwnd) {
                if let Some(since) = hung_since.take() {
                    logging!(
                        warn,
                        Type::Window,
                        "Интерфейс снова отвечает, не отвечал {} с",
                        since.elapsed().as_secs()
                    );
                }
                continue;
            }

            match hung_since {
                None => {
                    hung_since = Some(Instant::now());
                    reminded_at = Instant::now();
                    logging!(
                        error,
                        Type::Window,
                        "Главный поток перестал разбирать сообщения окна — интерфейс завис"
                    );
                }
                Some(since) if since.elapsed() >= RESTART_AFTER => {
                    restart_self();
                    hung_since = None;
                }
                Some(since) if reminded_at.elapsed() >= REMIND => {
                    reminded_at = Instant::now();
                    logging!(
                        error,
                        Type::Window,
                        "Интерфейс не отвечает уже {} с",
                        since.elapsed().as_secs()
                    );
                }
                Some(_) => {}
            }
        }
    }
}
