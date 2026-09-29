//! Размер окна меняет человек — признаком от системы, а не догадкой.
//!
//! Подгон окна под содержимое гаснет, когда человек сам берётся за высоту.
//! Раньше ручным считалось любое изменение размера, которое интерфейс не узнал
//! своим, и тумблер выключали смена монитора, RDP, раскладка Windows 11,
//! полноэкранный режим macOS. Теперь признак даёт система: на Windows —
//! перетаскивание края окна, на macOS — живое изменение размера. Snap и
//! тайлинг ручным не считаются. На Linux рамку рисует сам интерфейс, и
//! признак ставят его ручки (`window-controller.tsx`).
//!
//! Пока человек тянет край, подгон ничего не ставит (`a_person_is_resizing`):
//! иначе при смене одной ширины перенесённое содержимое меняло бы высоту
//! посреди перетаскивания, а это читалось бы как ручная смена высоты.

#[cfg(any(target_os = "windows", target_os = "macos"))]
use clash_verge_logging::{Type, logging, logging_error};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::{config::IVerge, core::handle, process::AsyncHandler};

/// Сообщений о перетаскивании приходят десятки. Выключают подгон по одному:
/// следующее уже видит выключенный тумблер.
#[cfg(any(target_os = "windows", target_os = "macos"))]
static TURNING_THE_FIT_OFF: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Человек взялся за высоту, а тумблер ещё не записан (патч настроек может
/// ждать чужого долгого патча). До записи подгон высоту не трогает.
#[cfg(any(target_os = "windows", target_os = "macos"))]
static THE_PERSON_TOOK_THE_HEIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn the_person_took_the_height() -> bool {
    THE_PERSON_TOOK_THE_HEIGHT.load(std::sync::atomic::Ordering::Acquire)
}

/// На Linux подгон выключается сразу по нажатию на ручку рамки.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const fn the_person_took_the_height() -> bool {
    false
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn the_person_changed_the_height() {
    THE_PERSON_TOOK_THE_HEIGHT.store(true, std::sync::atomic::Ordering::Release);
    AsyncHandler::spawn(|| async {
        let _one_at_a_time = TURNING_THE_FIT_OFF.lock().await;
        scopeguard::defer! {
            THE_PERSON_TOOK_THE_HEIGHT.store(false, std::sync::atomic::Ordering::Release);
        }
        if !super::window::window_fit_content_enabled().await {
            return;
        }
        logging!(
            info,
            Type::Window,
            "высоту окна меняет человек — подгон под содержимое выключен"
        );
        logging_error!(
            Type::Window,
            crate::feat::patch_verge(
                &IVerge {
                    window_fit_content: Some(false),
                    ..IVerge::default()
                },
                false,
            )
            .await
        );
        handle::Handle::refresh_verge();
    });
}

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{GetWindowRect, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY, WM_SIZING},
        },
    };

    const SUBCLASS_ID: usize = 0x636c_6f64;

    /// Окно в модальном цикле перетаскивания (края или заголовка): между
    /// `WM_ENTERSIZEMOVE` и `WM_EXITSIZEMOVE`, которые система шлёт парой.
    static IN_SIZE_MOVE: AtomicBool = AtomicBool::new(false);

    pub fn a_person_is_resizing() -> bool {
        IN_SIZE_MOVE.load(Ordering::Acquire)
    }

    /// `WM_SIZING` приходит, только пока человек тянет край окна (мышью или
    /// клавишами из системного меню); Snap и перестановки системы его не шлют.
    /// Подгон гаснет, если предложенная высота рамки отличается от нынешней.
    unsafe extern "system" fn on_message(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        _ref_data: usize,
    ) -> LRESULT {
        match message {
            WM_ENTERSIZEMOVE => IN_SIZE_MOVE.store(true, Ordering::Release),
            WM_EXITSIZEMOVE => IN_SIZE_MOVE.store(false, Ordering::Release),
            WM_SIZING if lparam != 0 => {
                // SAFETY: для WM_SIZING lparam указывает на RECT перетаскиваемой
                // рамки, живой на время обработки сообщения.
                let proposed = unsafe { &*(lparam as *const RECT) };
                let mut current = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                // SAFETY: hwnd — окно, которому пришло сообщение.
                let known = unsafe { GetWindowRect(hwnd, &raw mut current) } != 0;
                if known && proposed.bottom - proposed.top != current.bottom - current.top {
                    super::the_person_changed_the_height();
                }
            }
            WM_NCDESTROY => {
                IN_SIZE_MOVE.store(false, Ordering::Release);
                // SAFETY: снимаем свой же подкласс с уничтожаемого окна.
                unsafe { RemoveWindowSubclass(hwnd, Some(on_message), SUBCLASS_ID) };
            }
            _ => {}
        }
        // SAFETY: передаём сообщение дальше по цепочке подклассов окна.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    pub fn watch(window: &tauri::WebviewWindow) {
        use clash_verge_logging::{Type, logging};

        let hwnd = match window.hwnd() {
            Ok(hwnd) => hwnd.0 as isize,
            Err(e) => {
                logging!(warn, Type::Window, "признак ручного ресайза: окна нет: {}", e);
                return;
            }
        };
        // Подкласс ставится только из потока, которому принадлежит окно.
        let scheduled = window.run_on_main_thread(move || {
            // SAFETY: hwnd — живое главное окно; подкласс снимается на WM_NCDESTROY.
            let installed = unsafe { SetWindowSubclass(hwnd as HWND, Some(on_message), SUBCLASS_ID, 0) } != 0;
            if !installed {
                logging!(
                    warn,
                    Type::Window,
                    "признак ручного ресайза: подкласс окна не поставлен"
                );
            }
        });
        if let Err(e) = scheduled {
            logging!(
                warn,
                Type::Window,
                "признак ручного ресайза: главный поток недоступен: {}",
                e
            );
        }
    }
}

#[cfg(target_os = "windows")]
pub use platform::watch;

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::atomic::{AtomicU32, Ordering};

    use objc2_app_kit::NSWindow;

    static LAST_HEIGHT: AtomicU32 = AtomicU32::new(0);

    /// Живое изменение размера у macOS — только когда человек тянет край окна.
    /// Звать на главном потоке.
    fn in_live_resize(window: &tauri::WebviewWindow) -> bool {
        let Ok(pointer) = window.ns_window() else {
            return false;
        };
        if pointer.is_null() {
            return false;
        }
        // SAFETY: указатель — NSWindow этого окна, а зовут нас на главном
        // потоке, где с NSWindow и можно работать.
        let ns_window = unsafe { &*pointer.cast::<NSWindow>() };
        ns_window.inLiveResize()
    }

    /// Зовётся на главном потоке из события `Resized` главного окна.
    pub fn on_resized(window: &tauri::WebviewWindow, size: tauri::PhysicalSize<u32>) {
        let previous = LAST_HEIGHT.swap(size.height, Ordering::AcqRel);
        // Вход в полноэкранный режим и зум анимируются системой — это не
        // выбор размера человеком, что бы ни говорил флаг живого ресайза.
        if window.is_fullscreen().unwrap_or(false) || window.is_maximized().unwrap_or(false) {
            return;
        }
        if previous != 0 && previous != size.height && in_live_resize(window) {
            super::the_person_changed_the_height();
        }
    }

    pub async fn a_person_is_resizing(window: &tauri::WebviewWindow) -> bool {
        let (answer, asked) = tokio::sync::oneshot::channel();
        let on_main = window.clone();
        if window
            .run_on_main_thread(move || {
                let _ = answer.send(in_live_resize(&on_main));
            })
            .is_err()
        {
            return false;
        }
        asked.await.unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
pub use platform::on_resized;

/// Человек сейчас тянет край окна — подгону не время ставить высоту.
#[cfg(target_os = "windows")]
#[allow(clippy::unused_async)]
pub async fn a_person_is_resizing(_window: &tauri::WebviewWindow) -> bool {
    platform::a_person_is_resizing()
}

/// Человек сейчас тянет край окна — подгону не время ставить высоту.
#[cfg(target_os = "macos")]
pub async fn a_person_is_resizing(window: &tauri::WebviewWindow) -> bool {
    platform::a_person_is_resizing(window).await
}

/// На Linux край тянут наши ручки, и подгон выключается по нажатию на них.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[allow(clippy::unused_async)]
pub async fn a_person_is_resizing(_window: &tauri::WebviewWindow) -> bool {
    false
}
