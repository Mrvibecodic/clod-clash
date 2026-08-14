use crate::{core::handle, process::AsyncHandler, utils::resolve::window::build_new_window};
use clash_verge_limiter::Limiter;
use clash_verge_logging::{Type, logging};
use once_cell::sync::Lazy;
use std::pin::Pin;
use std::time::Duration;
use tauri::{Emitter as _, Manager as _, WebviewWindow, Wry};

/// Результат операции с окном
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowOperationResult {
    /// Окно показано и получило фокус
    Shown,
    /// Окно скрыто
    Hidden,
    /// Создано новое окно
    Created,
    /// Окно уничтожено
    Destroyed,
    /// Операция не удалась
    Failed,
    /// Действие не требуется
    NoAction,
}

/// Состояние окна
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowState {
    /// Окно видимо и в фокусе
    VisibleFocused,
    /// Окно видимо, но не в фокусе
    VisibleUnfocused,
    /// Окно свёрнуто
    Minimized,
    /// Окно скрыто
    Hidden,
    /// Окно не существует
    NotExist,
}

// Механизм защиты от дребезга операций с окном
const WINDOW_OPERATION_DEBOUNCE_MS: u64 = 625;
static WINDOW_OPERATION_LIMITER: Lazy<Limiter> = Lazy::new(|| {
    Limiter::new(
        Duration::from_millis(WINDOW_OPERATION_DEBOUNCE_MS),
        clash_verge_limiter::SystemClock,
    )
});

/// clod:freeze-restore — сколько ждать ответа от активации окна.
///
/// Каждый вызов окна из рабочего потока — рандеву с главным потоком, и ждёт он
/// БЕЗ срока: если главный поток встал (14.08 его подвесил усыплённый WebView2
/// при разворачивании), задача клика по трею висит вечно и молча. Срок не
/// оживляет окно — он даёт запись в лог вместо тишины и отпускает вызвавшего.
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(5);

fn should_handle_window_operation() -> bool {
    let allow = WINDOW_OPERATION_LIMITER.check();
    if !allow {
        logging!(debug, Type::Window, "window operation rate limited");
    }
    allow
}

/// Единый менеджер окон
pub struct WindowManager;

impl WindowManager {
    #[cfg(target_os = "macos")]
    fn set_macos_activation_policy_regular() {
        logging!(info, Type::Window, "Применяю специфичную для macOS политику активации");
        handle::Handle::global().set_activation_policy_regular();
    }

    pub fn get_main_window_with_state() -> (Option<WebviewWindow<Wry>>, WindowState) {
        let Some(window) = Self::get_main_window() else {
            return (None, WindowState::NotExist);
        };

        let is_minimized = window.is_minimized().unwrap_or(false);
        let is_visible = window.is_visible().unwrap_or(false);
        let is_focused = window.is_focused().unwrap_or(false);

        let state = if is_minimized {
            WindowState::Minimized
        } else if !is_visible {
            WindowState::Hidden
        } else if is_focused {
            WindowState::VisibleFocused
        } else {
            WindowState::VisibleUnfocused
        };

        (Some(window), state)
    }

    pub fn get_main_window_state() -> WindowState {
        match Self::get_main_window() {
            Some(window) => {
                let is_minimized = window.is_minimized().unwrap_or(false);
                let is_visible = window.is_visible().unwrap_or(false);
                let is_focused = window.is_focused().unwrap_or(false);

                if is_minimized {
                    return WindowState::Minimized;
                }

                if !is_visible {
                    return WindowState::Hidden;
                }

                if is_focused {
                    WindowState::VisibleFocused
                } else {
                    WindowState::VisibleUnfocused
                }
            }
            None => WindowState::NotExist,
        }
    }

    /// Получить экземпляр главного окна
    pub fn get_main_window() -> Option<WebviewWindow<Wry>> {
        let app_handle = handle::Handle::app_handle();
        app_handle.get_webview_window("main")
    }

    /// Умный показ главного окна
    pub async fn show_main_window() -> WindowOperationResult {
        // Проверка защиты от дребезга
        if !should_handle_window_operation() {
            return WindowOperationResult::NoAction;
        }

        logging!(info, Type::Window, "Начинаю умный показ главного окна");
        logging!(debug, Type::Window, "{}", Self::get_window_status_info());

        let current_state = Self::get_main_window_state();

        match current_state {
            WindowState::NotExist => {
                logging!(info, Type::Window, "Окно не существует, создаю новое окно");
                if Self::create_window(true).await {
                    logging!(info, Type::Window, "Окно создано успешно");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    WindowOperationResult::Created
                } else {
                    logging!(warn, Type::Window, "Не удалось создать окно");
                    WindowOperationResult::Failed
                }
            }
            WindowState::VisibleFocused => {
                logging!(info, Type::Window, "Окно уже видимо и в фокусе, действие не требуется");
                WindowOperationResult::NoAction
            }
            WindowState::VisibleUnfocused | WindowState::Minimized | WindowState::Hidden => {
                let (window, state_after_check) = Self::get_main_window_with_state();
                if state_after_check == WindowState::VisibleFocused {
                    logging!(info, Type::Window, "Окно за время проверки стало видимым и в фокусе");
                    return WindowOperationResult::NoAction;
                }
                if let Some(window) = window {
                    Self::activate_window_guarded(window).await
                } else {
                    WindowOperationResult::Failed
                }
            }
        }
    }

    /// clod:freeze-restore — активация со сроком ответа.
    ///
    /// Сама активация синхронная и блокирующая, поэтому уезжает в блокирующий
    /// поток: зависший вызов окна больше не морозит воркер рантайма, а по
    /// истечении срока в логе остаётся запись — раньше на этом месте была
    /// тишина, и понять, что приложение висит именно здесь, можно было только
    /// по ОТСУТСТВИЮ следующей строки.
    async fn activate_window_guarded(window: WebviewWindow<Wry>) -> WindowOperationResult {
        let task = AsyncHandler::spawn_blocking(move || Self::activate_window(&window));
        match tokio::time::timeout(ACTIVATE_TIMEOUT, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                logging!(warn, Type::Window, "Активация окна прервана: {}", e);
                WindowOperationResult::Failed
            }
            Err(_) => {
                logging!(
                    error,
                    Type::Window,
                    "Активация окна не ответила за {} с: главный поток не разбирает оконные вызовы",
                    ACTIVATE_TIMEOUT.as_secs()
                );
                WindowOperationResult::Failed
            }
        }
    }

    /// Переключить состояние показа главного окна (показать/скрыть)
    pub async fn toggle_main_window() -> WindowOperationResult {
        if !should_handle_window_operation() {
            return WindowOperationResult::NoAction;
        }

        let (window, state) = Self::get_main_window_with_state();

        logging!(debug, Type::Window, "Текущее состояние: {:?}", state);

        match state {
            WindowState::NotExist => Self::handle_not_exist_toggle().await,
            WindowState::VisibleFocused | WindowState::VisibleUnfocused => Self::hide_main_window(window.as_ref()),
            WindowState::Minimized | WindowState::Hidden => Self::activate_existing_main_window(window.as_ref()),
        }
    }

    // Окно не существует — создаём новое окно
    async fn handle_not_exist_toggle() -> WindowOperationResult {
        logging!(info, Type::Window, "Окно не существует, создаю новое окно");
        // Защита от дребезга уже есть, вызываем внутренний метод напрямую
        if Self::create_window(true).await {
            WindowOperationResult::Created
        } else {
            WindowOperationResult::Failed
        }
    }

    // Скрыть главное окно
    fn hide_main_window(window: Option<&WebviewWindow<Wry>>) -> WindowOperationResult {
        logging!(info, Type::Window, "Окно видимо, скрываю окно");
        if let Some(window) = window {
            match window.close() {
                Ok(_) => {
                    logging!(info, Type::Window, "Окно успешно скрыто");
                    WindowOperationResult::Hidden
                }
                Err(e) => {
                    logging!(warn, Type::Window, "Не удалось скрыть окно: {}", e);
                    WindowOperationResult::Failed
                }
            }
        } else {
            logging!(warn, Type::Window, "Не удалось получить экземпляр окна");
            WindowOperationResult::Failed
        }
    }

    // Активировать существующее главное окно
    fn activate_existing_main_window(window: Option<&WebviewWindow<Wry>>) -> WindowOperationResult {
        logging!(
            info,
            Type::Window,
            "Окно существует, но скрыто или свёрнуто, активирую окно"
        );
        if let Some(window) = window {
            Self::activate_window(window)
        } else {
            logging!(warn, Type::Window, "Не удалось получить экземпляр окна");
            WindowOperationResult::Failed
        }
    }

    /// Активировать окно (отменить сворачивание, показать, установить фокус)
    fn activate_window(window: &WebviewWindow<Wry>) -> WindowOperationResult {
        logging!(info, Type::Window, "Начинаю активацию окна");
        #[cfg(target_os = "macos")]
        Self::set_macos_activation_policy_regular();

        // Процесс рендеринга был завершён системой: сначала reload, показ и фокус
        // передаём в on_page_load(Finished). Показываем окно только когда контент готов,
        // чтобы избежать мерцания белого экрана. defer только при успешном reload, иначе показ напрямую.
        #[allow(unused_mut)]
        let mut defer_show_to_page_load = false;
        #[cfg(target_os = "macos")]
        if crate::utils::resolve::window::take_webview_needs_reload() {
            logging!(
                info,
                Type::Window,
                "Процесс рендеринга был завершён системой, перезагружаю страницу перед активацией окна"
            );
            match window.reload() {
                Ok(()) => defer_show_to_page_load = true,
                Err(e) => logging!(
                    warn,
                    Type::Window,
                    "Не удалось перезагрузить страницу, показываю окно напрямую: {}",
                    e
                ),
            }
        }

        let mut operations_successful = true;

        // clod:freeze-restore — каждый шаг отмечается ДО вызова.
        // 14.08 приложение зависло между «отменяю сворачивание» и «окно
        // активировано», а между этими записями было пять вызовов окна подряд:
        // по логу нельзя было сказать, какой именно не вернулся. Теперь можно.
        // 1. Если окно свёрнуто, сначала отменяем сворачивание
        if window.is_minimized().unwrap_or(false) {
            logging!(info, Type::Window, "Окно свёрнуто, отменяю сворачивание");
            if let Err(e) = window.unminimize() {
                logging!(warn, Type::Window, "Не удалось отменить сворачивание: {}", e);
                operations_successful = false;
            }
            logging!(debug, Type::Window, "Сворачивание отменено");
        }

        // 2/3. Показ + фокус (при ветке reload пропускается, передаётся в on_page_load)
        if !defer_show_to_page_load {
            logging!(debug, Type::Window, "Показываю окно");
            if let Err(e) = window.show() {
                logging!(warn, Type::Window, "Не удалось показать окно: {}", e);
                operations_successful = false;
            }
            logging!(debug, Type::Window, "Ставлю фокус");
            if let Err(e) = window.set_focus() {
                logging!(warn, Type::Window, "Не удалось установить фокус окна: {}", e);
                operations_successful = false;
            }
        }

        // clod:window-return — говорим странице, что окно показано, вместо
        // того чтобы ждать, пока она догадается сама. `document.hidden` под
        // Tauri врёт, события окна на разных платформах приходят по-разному, а
        // `is_visible()` для свёрнутого окна на Windows отвечает `true` — то
        // есть единственной безусловной проверкой оставался сторож раз в
        // минуту, и до него экран показывал цифры прошлого показа. Мы этот
        // момент знаем ТОЧНО: он прямо здесь.
        logging!(debug, Type::Window, "Сообщаю странице о показе");
        let _ = window.emit("verge://window-shown", ());

        if operations_successful {
            logging!(info, Type::Window, "Окно успешно активировано");
            WindowOperationResult::Shown
        } else {
            logging!(warn, Type::Window, "Активация окна частично не удалась");
            WindowOperationResult::Failed
        }
    }

    /// Проверить, видимо ли окно
    pub fn is_main_window_visible(window: Option<&WebviewWindow<Wry>>) -> bool {
        window.map(|w| w.is_visible().unwrap_or(false)).unwrap_or(false)
    }

    /// Проверить, в фокусе ли окно
    pub fn is_main_window_focused(window: Option<&WebviewWindow<Wry>>) -> bool {
        window.map(|w| w.is_focused().unwrap_or(false)).unwrap_or(false)
    }

    /// Проверить, свёрнуто ли окно
    pub fn is_main_window_minimized(window: Option<&WebviewWindow<Wry>>) -> bool {
        window.map(|w| w.is_minimized().unwrap_or(false)).unwrap_or(false)
    }

    /// Создать новое окно, защита от дребезга предотвращает повторные вызовы
    /// После создания окно остаётся скрытым, show вызывает фронтенд index.html
    /// после отрисовки overlay, чтобы избежать мерцания темы
    pub fn create_window(should_create: bool) -> Pin<Box<dyn Future<Output = bool> + Send>> {
        Box::pin(async move {
            logging!(
                info,
                Type::Window,
                "Начинаю создание главного окна, should_create={}",
                should_create
            );

            if !should_create {
                return false;
            }

            #[cfg(target_os = "macos")]
            Self::set_macos_activation_policy_regular();

            match build_new_window().await {
                Ok(_) => {
                    logging!(
                        info,
                        Type::Window,
                        "Новое окно успешно создано, ожидаю отрисовки фронтенда для показа"
                    );

                    true
                }
                Err(e) => {
                    logging!(error, Type::Window, "Не удалось создать новое окно: {}", e);
                    false
                }
            }
        })
    }

    /// Уничтожить окно
    pub fn destroy_main_window() -> WindowOperationResult {
        if let Some(window) = Self::get_main_window() {
            let _ = window.destroy();
            logging!(info, Type::Window, "Окно уничтожено");
            #[cfg(target_os = "macos")]
            {
                logging!(info, Type::Window, "Применяю специфичную для macOS политику активации");
                handle::Handle::global().set_activation_policy_accessory();
            }
            return WindowOperationResult::Destroyed;
        }
        WindowOperationResult::Failed
    }

    /// Получить подробную информацию о состоянии окна
    fn get_window_status_info() -> String {
        let (window, state) = Self::get_main_window_with_state();
        let is_visible = Self::is_main_window_visible(window.as_ref());
        let is_focused = Self::is_main_window_focused(window.as_ref());
        let is_minimized = Self::is_main_window_minimized(window.as_ref());

        format!("Состояние окна: {state:?} | Видимо: {is_visible} | В фокусе: {is_focused} | Свёрнуто: {is_minimized}")
    }
}
