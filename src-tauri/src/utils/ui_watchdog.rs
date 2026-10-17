#[cfg(target_os = "windows")]
pub use windows_watchdog::{responds, start};

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
    /// Зависание меряется промахами подряд, а не часами: `Instant` на Windows
    /// считает и сон, и одна проба до сна с одной после него сошли бы за
    /// зависание длиной в весь сон.
    const RESTART_AFTER_MISSES: u32 = 10;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    static PROBE_HWND: AtomicIsize = AtomicIsize::new(0);
    static STARTED: AtomicBool = AtomicBool::new(false);

    /// Начать смотреть за главным потоком. Вызывать в главном потоке: окно,
    /// которому сторож шлёт пробу, принадлежит потоку, где его создали.
    /// Повторный вызов ничего не делает.
    ///
    /// Проба идёт не главному окну, а своему невидимому окну без экрана. Главное
    /// окно в облегчённом режиме уничтожают — пропавшее окно на сообщения не
    /// отвечает, и это сошло бы за зависание; а пока главного окна нет,
    /// настоящее зависание было бы не видно. Своё окно живёт, пока живёт
    /// приложение.
    pub fn start() {
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(hwnd) = probe_window() else {
            STARTED.store(false, Ordering::SeqCst);
            logging!(warn, Type::Window, "Сторож интерфейса не получил окно для проверки");
            return;
        };
        PROBE_HWND.store(hwnd, Ordering::Relaxed);

        match std::thread::Builder::new().name("ui-watchdog".into()).spawn(watch_loop) {
            Ok(_) => logging!(info, Type::Window, "Сторож интерфейса запущен"),
            Err(e) => logging!(warn, Type::Window, "Не удалось запустить сторож интерфейса: {}", e),
        }
    }

    /// Невидимое окно только для сообщений, в текущем потоке.
    fn probe_window() -> Option<isize> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, HWND_MESSAGE};

        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        (!hwnd.is_null()).then_some(hwnd as isize)
    }

    pub fn responds() -> bool {
        let hwnd = PROBE_HWND.load(Ordering::Relaxed);
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
                    "Интерфейс не ответил {} проверок подряд, приложение перезапускается",
                    RESTART_AFTER_MISSES
                );
                crate::feat::tidy_up_before_an_abrupt_end("перезапуском");
                log::logger().flush();
                std::thread::sleep(Duration::from_millis(200));
                // Не ноль: аварийный перезапуск не должен выглядеть штатным выходом.
                std::process::exit(1);
            }
            Err(e) => logging!(error, Type::Window, "Не удалось запланировать перезапуск: {}", e),
        }
    }

    const fn misses_after(misses: u32, answered: bool) -> u32 {
        if answered { 0 } else { misses.saturating_add(1) }
    }

    fn watch_loop() {
        let mut misses = 0_u32;
        let mut hung_since = Instant::now();
        let mut reminded_at = Instant::now();

        loop {
            std::thread::sleep(POLL);

            // Выход держит главный поток уборкой, и это не зависание:
            // перезапуск посреди выхода поднял бы приложение снова. Выход
            // могут и отменить — тогда сторож продолжает.
            if crate::core::handle::Handle::global().is_exiting() {
                misses = 0;
                continue;
            }

            let before = misses;
            misses = misses_after(misses, window_answers(PROBE_HWND.load(Ordering::Relaxed)));
            match misses {
                0 if before > 0 => logging!(
                    warn,
                    Type::Window,
                    "Интерфейс снова отвечает, не отвечал {} с",
                    hung_since.elapsed().as_secs()
                ),
                0 => {}
                1 => {
                    hung_since = Instant::now();
                    reminded_at = Instant::now();
                    logging!(
                        error,
                        Type::Window,
                        "Главный поток перестал разбирать сообщения окна — интерфейс завис"
                    );
                }
                _ if misses >= RESTART_AFTER_MISSES => {
                    restart_self();
                    misses = 0;
                }
                _ if reminded_at.elapsed() >= REMIND => {
                    reminded_at = Instant::now();
                    logging!(
                        error,
                        Type::Window,
                        "Интерфейс не отвечает уже {} с",
                        hung_since.elapsed().as_secs()
                    );
                }
                _ => {}
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{RESTART_AFTER_MISSES, misses_after, probe_window, window_answers};

        #[test]
        fn only_misses_in_a_row_lead_to_a_restart() {
            let mut misses = 0;
            for _ in 0..RESTART_AFTER_MISSES - 1 {
                misses = misses_after(misses, false);
            }
            assert!(misses < RESTART_AFTER_MISSES);
            assert_eq!(misses_after(misses, true), 0);
            assert_eq!(misses_after(misses, false), RESTART_AFTER_MISSES);
        }
        use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

        /// Окно для пробы создаётся без экрана и отвечает, пока его поток
        /// разбирает сообщения (из своего же потока ответ приходит сразу).
        #[test]
        fn the_probe_window_answers_for_its_thread() {
            let hwnd = probe_window();
            assert!(hwnd.is_some(), "окно для пробы не создалось");
            let Some(hwnd) = hwnd else { return };
            assert!(window_answers(hwnd));
            unsafe { DestroyWindow(hwnd as *mut core::ffi::c_void) };
        }
    }
}
