use crate::core::handle;
use crate::process::AsyncHandler;
use crate::utils::{connections_stream, tray_speed};
use crate::{Type, logging};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tauri::async_runtime::JoinHandle;
use tauri_plugin_mihomo::models::ConnectionId;

/// Интервал переподключения после сбоя потока скорости в трее.
const TRAY_SPEED_RETRY_DELAY: Duration = Duration::from_secs(1);
/// Интервал холостого опроса во время работы потока скорости в трее.
const TRAY_SPEED_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Если поток скорости в трее не получает данных за это время,
/// запускается переподключение и деградация до 0/0.
const TRAY_SPEED_STALE_TIMEOUT: Duration = Duration::from_secs(5);

/// Контроллер задачи скорости в трее для macOS.
#[derive(Clone)]
pub struct TraySpeedController {
    speed_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    speed_connection_id: Arc<Mutex<Option<ConnectionId>>>,
}

impl Default for TraySpeedController {
    fn default() -> Self {
        Self {
            speed_task: Arc::new(Mutex::new(None)),
            speed_connection_id: Arc::new(Mutex::new(None)),
        }
    }
}

impl TraySpeedController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_task(&self, enable_tray_speed: bool) {
        if enable_tray_speed {
            self.start_task();
        } else {
            self.stop_task();
        }
    }

    /// Запускает фоновую задачу сбора скорости для трея (на основе
    /// WebSocket-потока `/traffic`).
    fn start_task(&self) {
        if handle::Handle::global().is_exiting() {
            return;
        }

        // Ключевой шаг: не запускаем задачу скорости, если трей недоступен,
        // чтобы избежать бесполезных повторов подключения.
        if !Self::has_main_tray() {
            logging!(
                warn,
                Type::Tray,
                "трей недоступен, запуск задачи скорости в трее пропущен"
            );
            return;
        }

        let mut guard = self.speed_task.lock();
        if guard.as_ref().is_some_and(|task| !task.inner().is_finished()) {
            return;
        }

        let speed_connection_id = Arc::clone(&self.speed_connection_id);
        let task = AsyncHandler::spawn(move || async move {
            loop {
                if handle::Handle::global().is_exiting() {
                    break;
                }

                if !Self::has_main_tray() {
                    logging!(
                        warn,
                        Type::Tray,
                        "трей стал недоступен, задача скорости в трее остановлена"
                    );
                    break;
                }

                let stream_connect_result = connections_stream::connect_traffic_stream().await;
                let mut speed_stream = match stream_connect_result {
                    Ok(stream) => stream,
                    Err(err) => {
                        logging!(
                            debug,
                            Type::Tray,
                            "не удалось подключиться к потоку скорости трея, повтор позже: {err}"
                        );
                        Self::apply_tray_speed(0, 0);
                        tokio::time::sleep(TRAY_SPEED_RETRY_DELAY).await;
                        continue;
                    }
                };

                Self::set_speed_connection_id(&speed_connection_id, Some(speed_stream.connection_id));

                loop {
                    let next_state = speed_stream
                        .next_event(TRAY_SPEED_IDLE_POLL_INTERVAL, TRAY_SPEED_STALE_TIMEOUT, || {
                            handle::Handle::global().is_exiting()
                        })
                        .await;

                    match next_state {
                        connections_stream::StreamConsumeState::Event(speed_event) => {
                            Self::apply_tray_speed(speed_event.up, speed_event.down);
                        }
                        connections_stream::StreamConsumeState::Stale => {
                            logging!(
                                debug,
                                Type::Tray,
                                "поток скорости трея долго не получает данные, переподключение"
                            );
                            Self::apply_tray_speed(0, 0);
                            break;
                        }
                        connections_stream::StreamConsumeState::Closed
                        | connections_stream::StreamConsumeState::ExitRequested => {
                            break;
                        }
                    }
                }

                Self::disconnect_speed_connection(&speed_connection_id).await;

                if handle::Handle::global().is_exiting() || !Self::has_main_tray() {
                    break;
                }

                // Ветка Stale уже сбросила значение до 0/0 во внутреннем loop;
                // здесь подстраховка для ветки Closed (поток закрыт удалённой стороной).
                Self::apply_tray_speed(0, 0);
                tokio::time::sleep(TRAY_SPEED_RETRY_DELAY).await;
            }

            Self::set_speed_connection_id(&speed_connection_id, None);
        });

        *guard = Some(task);
    }

    /// Останавливает фоновую задачу сбора скорости для трея и очищает
    /// отображение скорости.
    fn stop_task(&self) {
        // Забираем дескриптор задачи, передаём его в задачу очистки вместе
        // со speed_connection_id.
        let task = self.speed_task.lock().take();
        let speed_connection_id = Arc::clone(&self.speed_connection_id);

        AsyncHandler::spawn(move || async move {
            // Ключевой шаг: сначала дожидаемся завершения abort, затем
            // разрываем WebSocket-соединение.
            // Если сразу после abort вызвать disconnect, задача может уже
            // забрать connection_id через take, но ещё не завершить разрыв —
            // это приведёт к потере connection_id и утечке соединения.
            // await дескриптора задачи гарантирует, что исходная задача
            // завершилась и connection_id больше не занят.
            if let Some(task) = task {
                task.abort();
                let _ = task.await;
            }
            Self::disconnect_speed_connection(&speed_connection_id).await;
        });

        let app_handle = handle::Handle::app_handle();
        if let Some(tray) = app_handle.tray_by_id(super::TRAY_ID) {
            let result = tray.with_inner_tray_icon(|inner| {
                if let Some(status_item) = inner.ns_status_item() {
                    tray_speed::clear_speed_attributed_title(&status_item);
                }
            });
            if let Err(err) = result {
                logging!(
                    warn,
                    Type::Tray,
                    "не удалось очистить форматированный текст скорости: {err}"
                );
            }
        }
    }

    fn has_main_tray() -> bool {
        handle::Handle::app_handle().tray_by_id(super::TRAY_ID).is_some()
    }

    fn set_speed_connection_id(
        speed_connection_id: &Arc<Mutex<Option<ConnectionId>>>,
        connection_id: Option<ConnectionId>,
    ) {
        *speed_connection_id.lock() = connection_id;
    }

    fn take_speed_connection_id(speed_connection_id: &Arc<Mutex<Option<ConnectionId>>>) -> Option<ConnectionId> {
        speed_connection_id.lock().take()
    }

    async fn disconnect_speed_connection(speed_connection_id: &Arc<Mutex<Option<ConnectionId>>>) {
        if let Some(connection_id) = Self::take_speed_connection_id(speed_connection_id) {
            connections_stream::disconnect_connection(connection_id).await;
        }
    }

    fn apply_tray_speed(up: u64, down: u64) {
        let app_handle = handle::Handle::app_handle();
        if let Some(tray) = app_handle.tray_by_id(super::TRAY_ID) {
            let result = tray.with_inner_tray_icon(move |inner| {
                if let Some(status_item) = inner.ns_status_item() {
                    tray_speed::set_speed_attributed_title(&status_item, up, down);
                }
            });
            if let Err(err) = result {
                logging!(
                    warn,
                    Type::Tray,
                    "не удалось установить форматированный текст скорости: {err}"
                );
            }
        }
    }
}
