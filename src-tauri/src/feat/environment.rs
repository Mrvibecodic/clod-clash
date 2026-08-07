//! clod:wake-net — сон машины и смена сети.
//!
//! Оба события разрывают то, что приложение считает настроенным, и ни одно из
//! них ничем себя не объявляет:
//!
//!   * после пробуждения системный прокси может быть уже не наш (Windows
//!     восстанавливает профиль сети), соединения ядра держат мёртвые сокеты, а
//!     маршрут TUN исчезает вместе с интерфейсом;
//!   * при смене сети (Wi-Fi → кабель, новая точка доступа, VPN работодателя)
//!     то же самое, только без паузы.
//!
//! Оба увидены одним сторожем и без платформенных подписок на события: сон
//! виден по скачку настенных часов относительно тика, смена сети — по составу
//! поднятых интерфейсов и их адресов. Работает одинаково на Windows, macOS и
//! Linux, а собственных прав не требует вовсе.
//!
//! Разбор соседей: Koala перезапускает ядро по пропаданию сети
//! (`src/main/core/manager.ts:643`) и переключает режим по смене SSID,
//! FlClashX слушает `onConnectivityChanged` и закрывает соединения
//! (`lib/application.dart:110`). У нас не было ни того ни другого.

use crate::{
    config::Config,
    constants::timing,
    core::{handle, sysopt::Sysopt},
    process::AsyncHandler,
};
use clash_verge_logging::{Type, logging};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};

static WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);

/// Насколько настенные часы должны обогнать монотонные, чтобы это была не
/// погрешность планировщика, а сон.
///
/// Тик короткий, поэтому обычное расхождение — десятки миллисекунд. Порог взят
/// с большим запасом: занятая машина может задержать задачу на секунды, и
/// принимать это за пробуждение не стоит — сверка не бесплатна.
const SLEEP_SLACK: Duration = Duration::from_secs(20);

/// Отпечаток сети: какие интерфейсы подняты и с какими адресами.
///
/// Именно адреса, а не одни имена: переезд с одной точки доступа на другую
/// оставляет имя интерфейса прежним, а меняет ровно адрес — и это самый частый
/// случай из всех, которые мы ловим. `BTreeSet` даёт устойчивый порядок,
/// поэтому перестановка интерфейсов в выдаче системы за смену сети не считается.
fn network_fingerprint() -> BTreeSet<std::string::String> {
    let Ok(interfaces) = crate::cmd::network::get_network_interfaces_info() else {
        // Спросить не вышло. Пустой отпечаток сравнялся бы с прошлым и дал
        // ложную «смену сети» на следующем круге, поэтому возвращаем маркер:
        // он равен сам себе и ничего не запускает.
        return BTreeSet::from([std::string::String::from("<unknown>")]);
    };

    interfaces
        .into_iter()
        .flat_map(|interface| {
            let name = interface.name.clone();
            interface
                .addr
                .into_iter()
                .map(move |addr| match addr {
                    network_interface::Addr::V4(v4) => format!("{name}:{}", v4.ip),
                    network_interface::Addr::V6(v6) => format!("{name}:{}", v6.ip),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Что делать, когда окружение под приложением поменялось.
///
/// Намеренно только то, что дёшево и не может сделать хуже. Перезапуск ядра
/// сюда НЕ входит: под службой он стоит разрыва всего трафика, а живое ядро
/// после пробуждения обычно в порядке — ломаются его соединения, а не оно само.
/// Если ядро всё-таки умерло, это увидят те, чья это работа: сторож службы
/// (`core::manager::state`) и чтение вывода сайдкара.
async fn reconcile(reason: &str) {
    logging!(info, Type::Core, "[clod] environment changed ({reason}), reconciling");

    // 1. Системный прокси. Windows умеет вернуть настройки сети к профилю,
    //    записанному до сна, и наш прокси из них просто исчезает — молча.
    //    `update_sysproxy` идемпотентен: если всё на месте, он ничего не делает.
    let wants_sysproxy = Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false);
    if wants_sysproxy && let Err(e) = Sysopt::global().update_sysproxy().await {
        logging!(warn, Type::Core, "[clod] failed to re-assert the system proxy: {e}");
    }

    // 2. Соединения ядра. Через сон живут только записи о них: сокеты по ту
    //    сторону давно закрыты, а ядро узнает об этом лишь по таймауту — всё
    //    это время приложения пользователя ждут ответа в никуда. Приём взят у
    //    FlClashX (`lib/application.dart:110`).
    if let Err(e) = handle::Handle::mihomo().await.close_all_connections().await {
        logging!(
            debug,
            Type::Core,
            "[clod] could not close connections after the environment changed: {e}"
        );
    }
}

/// clod:wake-net — запустить сторож окружения. Идемпотентно.
pub fn spawn_environment_watchdog() {
    if WATCHDOG_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }

    AsyncHandler::spawn(|| async {
        let mut last_tick = Instant::now();
        let mut last_wall = SystemTime::now();
        let mut last_network = network_fingerprint();

        loop {
            tokio::time::sleep(timing::ENVIRONMENT_TICK).await;
            if handle::Handle::global().is_exiting() {
                return;
            }

            let now_tick = Instant::now();
            let now_wall = SystemTime::now();

            // Сон засчитываем по РАСХОЖДЕНИЮ двух часов, а не по одному скачку
            // настенных: перевод часов руками (или синхронизация времени после
            // загрузки) двигает только настенные и сном не является, но и
            // монотонные во время сна на части систем стоят. Считаем сном
            // случай, когда настенные обогнали монотонные больше чем на запас.
            let wall_delta = now_wall.duration_since(last_wall).unwrap_or_default();
            let tick_delta = now_tick.duration_since(last_tick);
            let slept = wall_delta.saturating_sub(tick_delta) > SLEEP_SLACK;

            let network = network_fingerprint();
            let network_changed = network != last_network;

            last_tick = now_tick;
            last_wall = now_wall;
            last_network = network;

            let reason = match (slept, network_changed) {
                (true, true) => "woke up, network differs",
                (true, false) => "woke up",
                (false, true) => "network changed",
                (false, false) => continue,
            };

            reconcile(reason).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::SLEEP_SLACK;
    use crate::constants::timing;
    use std::time::Duration;

    /// Порог обязан быть заметно больше тика, иначе обычная задержка занятой
    /// машины читалась бы как пробуждение и сверка шла бы вхолостую каждый круг.
    #[test]
    fn the_sleep_threshold_leaves_room_for_a_busy_machine() {
        assert!(SLEEP_SLACK > timing::ENVIRONMENT_TICK);
    }

    /// Правило распознавания сна, отдельно от таймеров: настенные часы обогнали
    /// монотонные больше чем на запас.
    #[test]
    fn sleep_is_the_gap_between_two_clocks() {
        let slept = |wall: Duration, tick: Duration| wall.saturating_sub(tick) > SLEEP_SLACK;

        // Обычный круг: часы идут вместе.
        assert!(!slept(
            timing::ENVIRONMENT_TICK + Duration::from_millis(40),
            timing::ENVIRONMENT_TICK
        ));
        // Машина была занята и задержала задачу — оба счётчика выросли вместе.
        assert!(!slept(Duration::from_secs(45), Duration::from_secs(45)));
        // Крышку закрыли на час: настенные ушли, монотонные — нет.
        assert!(slept(Duration::from_secs(3600), timing::ENVIRONMENT_TICK));
    }
}
