use crate::{Type, core::handle, logging};
use anyhow::Result;
use serde::Deserialize;
use std::time::Duration;
use tauri_plugin_mihomo::models::ConnectionId;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Ёмкость ограниченной очереди потока Mihomo WebSocket, предотвращает
/// неограниченный рост памяти в нештатных ситуациях.
const MIHOMO_WS_STREAM_BUFFER_SIZE: usize = 8;
const MIHOMO_WS_STREAM_FORCE_CLOSE_WAIT_MS: u64 = 1000;

/// Событие мгновенной скорости `/traffic` (байт/сек).
#[derive(Debug, Clone, Copy)]
pub struct TrafficSpeedEvent {
    pub up: u64,
    pub down: u64,
}

/// Состояние потребления потока Mihomo WebSocket.
pub enum StreamConsumeState<T> {
    /// Получено одно бизнес-событие.
    Event(T),
    /// Соединение закрыто или поток сообщений завершился.
    Closed,
    /// Не получено ни одного валидного события за таймаут, нужно переподключение.
    Stale,
    /// Вышестоящий код запросил выход из цикла потребления.
    ExitRequested,
}

enum InternalWsEvent<T> {
    Data(T),
}

/// Дескриптор подписки Mihomo WebSocket (общий поток событий).
pub struct MihomoWsEventStream<T> {
    /// ID текущего подключения подписки, для принудительного отключения.
    pub connection_id: ConnectionId,
    /// Приёмник сообщений текущей подписки.
    receiver: mpsc::Receiver<InternalWsEvent<T>>,
    /// Метка времени последнего валидного события.
    last_valid_event_at: Instant,
}

#[derive(Deserialize)]
struct TrafficPayload {
    up: u64,
    down: u64,
}

fn parse_traffic_event(data: &[u8]) -> Option<InternalWsEvent<TrafficSpeedEvent>> {
    let payload = serde_json::from_slice::<TrafficPayload>(data).ok()?;
    Some(InternalWsEvent::Data(TrafficSpeedEvent {
        up: payload.up,
        down: payload.down,
    }))
}

fn try_send_internal_event<T>(message_tx: &mpsc::Sender<InternalWsEvent<T>>, event: InternalWsEvent<T>) {
    if let Err(err) = message_tx.try_send(event) {
        match err {
            // Очередь заполнена, событие отбрасывается, следующее событие всё равно перезапишет данные.
            tokio::sync::mpsc::error::TrySendError::Full(_) => {}
            // Канал может быть закрыт после завершения задачи, просто игнорируем.
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {}
        }
    }
}

/// Установить подписку WebSocket `/traffic` (общий поток).
pub async fn connect_traffic_stream() -> Result<MihomoWsEventStream<TrafficSpeedEvent>> {
    // Используем ограниченный mpsc-канал для приёма событий из колбэка, ограничивая накопление сообщений.
    let (message_tx, message_rx) = mpsc::channel::<InternalWsEvent<TrafficSpeedEvent>>(MIHOMO_WS_STREAM_BUFFER_SIZE);
    // Устанавливаем подписку WebSocket `/traffic` Mihomo.
    let connection_id = handle::Handle::mihomo()
        .await
        .ws_traffic({
            let message_tx = message_tx.clone();
            move |message| {
                if let Some(event) = parse_traffic_event(&message) {
                    try_send_internal_event(&message_tx, event);
                }
            }
        })
        .await?;
    drop(message_tx);
    Ok(MihomoWsEventStream {
        connection_id,
        receiver: message_rx,
        last_valid_event_at: Instant::now(),
    })
}

impl<T> MihomoWsEventStream<T> {
    /// Ждать следующее доступное событие или состояние завершения.
    ///
    /// # Arguments
    /// * `idle_poll_interval` - интервал проверки в режиме простоя
    /// * `stale_timeout` - таймаут отсутствия валидных событий
    /// * `should_exit` - функция проверки выхода со стороны вызывающего кода
    pub async fn next_event<F>(
        &mut self,
        _idle_poll_interval: Duration, // сигнатура сохранена, но внутренняя логика перешла на более эффективный механизм
        stale_timeout: Duration,
        should_exit: F,
    ) -> StreamConsumeState<T>
    where
        F: Fn() -> bool,
    {
        let sleep = tokio::time::sleep(stale_timeout);
        tokio::pin!(sleep);

        loop {
            if should_exit() {
                return StreamConsumeState::ExitRequested;
            }

            tokio::select! {
                maybe_event = self.receiver.recv() => {
                    match maybe_event {
                        Some(InternalWsEvent::Data(event)) => {
                            self.last_valid_event_at = Instant::now();
                            sleep.as_mut().reset(self.last_valid_event_at + stale_timeout);
                            return StreamConsumeState::Event(event);
                        }
                        None => return StreamConsumeState::Closed,
                    }
                }
                _ = &mut sleep => {
                    if self.last_valid_event_at.elapsed() >= stale_timeout {
                        return StreamConsumeState::Stale;
                    }
                    sleep.as_mut().reset(self.last_valid_event_at + stale_timeout);
                }
            }
        }
    }
}

/// Отключить указанное соединение Mihomo WebSocket.
///
/// # Arguments
/// * `connection_id` - ID целевого соединения
pub async fn disconnect_connection(connection_id: ConnectionId) {
    if let Err(err) = handle::Handle::mihomo()
        .await
        .disconnect(connection_id, Some(MIHOMO_WS_STREAM_FORCE_CLOSE_WAIT_MS))
        .await
    {
        logging!(
            debug,
            Type::Tray,
            "не удалось отключить подключение Mihomo WebSocket: {err}"
        );
    }
}
