//! clod:freeze-restore — сторож главного потока.
//!
//! 14.08 приложение дважды встало намертво при развороте свёрнутого окна, и
//! оба раза лог обрывался тишиной: как только главный поток перестаёт
//! разбирать оконные вызовы, каждая задача рантайма, которая трогает окно,
//! ложится на нём без срока — включая ту, что должна была записать «активация
//! не ответила». Свидетеля не осталось.
//!
//! Поэтому сторож живёт в СОБСТВЕННОМ системном потоке: он не зависит ни от
//! рантайма, ни от окна и спрашивает у Windows ровно то же, что спрашивает сам
//! проводник, когда рисует «Не отвечает» — разбирает ли поток окна очередь
//! сообщений. Ответ уходит в лог с точным временем, так что в следующий раз
//! видно, когда именно интерфейс умер и оживал ли он потом.

/// Взять окно под наблюдение. Зовётся сразу после создания окна.
#[cfg(not(target_os = "windows"))]
pub const fn watch(_window: &tauri::WebviewWindow) {}

/// Отвечает ли сейчас поток окна. Там, где спросить нельзя, считаем, что да.
#[cfg(not(target_os = "windows"))]
pub const fn responds() -> bool {
    true
}

#[cfg(target_os = "windows")]
pub use windows_watchdog::{responds, watch};

#[cfg(target_os = "windows")]
mod windows_watchdog {
    use std::{
        sync::atomic::{AtomicBool, AtomicIsize, Ordering},
        time::{Duration, Instant},
    };

    use clash_verge_logging::{Type, logging};

    /// Как часто спрашиваем окно.
    const POLL: Duration = Duration::from_secs(2);
    /// Сколько ждём ответа. Windows считает окно зависшим после 5 с, нам хватает
    /// меньшего: пауза здесь — это только задержка следующей проверки.
    const ANSWER: Duration = Duration::from_millis(1500);
    /// Как часто напоминать в логе, что интерфейс всё ещё не отвечает.
    const REMIND: Duration = Duration::from_secs(30);

    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
    static STARTED: AtomicBool = AtomicBool::new(false);

    /// Взять окно под наблюдение. Зовётся сразу после создания окна.
    pub fn watch(window: &tauri::WebviewWindow) {
        match window.hwnd() {
            Ok(hwnd) => {
                MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
                start();
            }
            Err(e) => logging!(warn, Type::Window, "Сторож интерфейса не получил окно: {}", e),
        }
    }

    /// Отвечает ли сейчас поток окна. Пока окна нет — считаем, что да, иначе
    /// проверка начнёт врать до его создания.
    pub fn responds() -> bool {
        let hwnd = MAIN_HWND.load(Ordering::Relaxed);
        hwnd == 0 || window_answers(hwnd)
    }

    /// Отвечает ли поток окна на сообщения.
    ///
    /// `WM_NULL` ничего не делает — важен сам факт ответа. `SMTO_ABORTIFHUNG`
    /// возвращает управление сразу, если Windows уже считает окно зависшим, а не
    /// досиживает срок до конца.
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
