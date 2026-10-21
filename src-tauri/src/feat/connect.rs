//! clod: что поднято сразу после запуска приложения.
//!
//! Подключение — действие пользователя, а не следствие старта. Флаги
//! «поднято сейчас» (`enable_system_proxy`, `enable_tun_mode`) переживают
//! перезапуск, и клиент прописывал системный прокси в настройки Windows сам,
//! ещё до того как показалось окно: пользователь видел чужую настройку,
//! которую он не включал. Теперь состояние подключения при старте задаётся
//! заново — по умолчанию выключенное, — и только «Подключаться при запуске»
//! повторяет нажатие кнопки.

use crate::config::{Config, IVerge, PrfItem};

/// Что просит поднимать провайдер (`clod-connect-mode`). Закрытый список: до
/// профиля доносятся только эти три значения, всё прочее отброшено разбором.
fn provider_targets(mode: &str) -> Option<(bool, bool)> {
    match mode {
        "tun" => Some((false, true)),
        "proxy" => Some((true, false)),
        "both" => Some((true, true)),
        _ => None,
    }
}

/// Способ подключения, который провайдер запер (`clod-lock-mode` вместе с
/// `clod-connect-mode`): `None` — замка на способ нет.
fn locked_targets(item: &PrfItem) -> Option<(bool, bool)> {
    if !item.lock_mode.unwrap_or(false) {
        return None;
    }
    item.connect_mode.as_deref().and_then(provider_targets)
}

/// `(системный прокси, TUN)` — что разрешено включать: цель вне замка
/// провайдера из приложения не включается, её можно только выключить.
pub(crate) fn allowed_targets(item: Option<&PrfItem>) -> (bool, bool) {
    item.and_then(locked_targets).unwrap_or((true, true))
}

/// То же для текущей подписки.
pub async fn current_allowed_targets() -> (bool, bool) {
    let profiles = Config::profiles().await.latest_arc();
    allowed_targets(profiles.current.as_ref().and_then(|uid| profiles.get_item(uid).ok()))
}

/// Способ подключения: выбор пользователя → пожелание панели → умолчание
/// приложения. При `clod-lock-mode` побеждает панель — иначе заголовок
/// «только TUN» не сработал бы ни у кого, кто однажды трогал тумблеры.
///
/// Зеркало `connectTargets` на фронте (таблица примеров общая), включая
/// последнее правило: способ не может быть пустым, системный прокси
/// возвращается сам.
pub(crate) fn connect_targets(verge: &IVerge, item: &PrfItem) -> (bool, bool) {
    let locked = locked_targets(item);
    let provider = item.connect_mode.as_deref().and_then(provider_targets);

    let sys = locked
        .map(|p| p.0)
        .or(verge.connect_system_proxy)
        .or_else(|| provider.map(|p| p.0))
        .unwrap_or(IVerge::DEFAULT_CONNECT_SYSTEM_PROXY);
    let tun = locked
        .map(|p| p.1)
        .or(verge.connect_tun_mode)
        .or_else(|| provider.map(|p| p.1))
        .unwrap_or(IVerge::DEFAULT_CONNECT_TUN_MODE);

    (sys || !tun, tun)
}

/// `(системный прокси, TUN)` — что должно быть поднято сразу после запуска.
///
/// Без «Подключаться при запуске» — ничего. Без подписки — тоже ничего, даже
/// с включённой настройкой: маршрутизировать нечего, а прописанный в системе
/// прокси уводил бы весь трафик машины в порт, за которым нет ни одного
/// сервера.
pub async fn launch_connect_state() -> (bool, bool) {
    let verge = Config::verge().await.latest_arc();
    if !verge.connect_on_launch.unwrap_or(false) {
        return (false, false);
    }

    let profiles = Config::profiles().await.latest_arc();
    let Some(item) = profiles.current.as_ref().and_then(|uid| profiles.get_item(uid).ok()) else {
        return (false, false);
    };

    connect_targets(&verge, item)
}

#[cfg(test)]
mod tests {
    use super::{allowed_targets, connect_targets, locked_targets, provider_targets};
    use crate::config::{IVerge, PrfItem};

    fn verge(sys: Option<bool>, tun: Option<bool>) -> IVerge {
        IVerge {
            connect_system_proxy: sys,
            connect_tun_mode: tun,
            ..Default::default()
        }
    }

    fn profile(mode: Option<&str>, locked: bool) -> PrfItem {
        PrfItem {
            connect_mode: mode.map(Into::into),
            lock_mode: Some(locked),
            ..Default::default()
        }
    }

    #[test]
    fn without_any_choice_the_system_proxy_is_the_default_target() {
        assert_eq!(
            connect_targets(&verge(None, None), &profile(None, false)),
            (true, false)
        );
    }

    #[test]
    fn the_users_own_choice_wins_on_an_unlocked_profile() {
        assert_eq!(
            connect_targets(&verge(Some(false), Some(true)), &profile(Some("proxy"), false)),
            (false, true)
        );
    }

    #[test]
    fn a_locked_profile_follows_the_provider_over_the_user() {
        assert_eq!(
            connect_targets(&verge(Some(true), Some(false)), &profile(Some("tun"), true)),
            (false, true)
        );
        assert_eq!(
            connect_targets(&verge(Some(false), Some(true)), &profile(Some("proxy"), true)),
            (true, false)
        );
        assert_eq!(
            connect_targets(&verge(None, None), &profile(Some("both"), true)),
            (true, true)
        );
    }

    #[test]
    fn a_lock_without_a_named_method_leaves_the_choice_to_the_user() {
        assert_eq!(
            connect_targets(&verge(Some(false), Some(true)), &profile(None, true)),
            (false, true)
        );
    }

    #[test]
    fn the_connection_method_can_never_end_up_empty() {
        assert_eq!(
            connect_targets(&verge(Some(false), Some(false)), &profile(None, false)),
            (true, false)
        );
    }

    #[test]
    fn only_a_locked_method_takes_a_target_away_from_the_tray_and_the_hotkey() {
        assert_eq!(allowed_targets(Some(&profile(Some("tun"), true))), (false, true));
        assert_eq!(allowed_targets(Some(&profile(Some("proxy"), true))), (true, false));
        assert_eq!(allowed_targets(Some(&profile(Some("both"), true))), (true, true));
        assert_eq!(allowed_targets(Some(&profile(Some("tun"), false))), (true, true));
        assert_eq!(allowed_targets(Some(&profile(None, true))), (true, true));
        assert_eq!(allowed_targets(None), (true, true));
    }

    #[derive(serde::Deserialize)]
    struct Case {
        verge: (Option<bool>, Option<bool>),
        profile: (Option<String>, bool),
        targets: (bool, bool),
        locked: Option<(bool, bool)>,
    }

    /// Примеры общие с `src/utils/connect-targets.test.ts`: окно, трей и
    /// горячие клавиши знают о способе подключения и замке одно и то же.
    #[test]
    fn the_window_counts_the_targets_and_the_lock_as_the_backend_does() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../../src/utils/connect-targets-cases.json")).unwrap_or_default();
        assert!(!cases.is_empty());
        for case in cases {
            let item = profile(case.profile.0.as_deref(), case.profile.1);
            let label = format!("{:?} {:?}", case.verge, case.profile);
            assert_eq!(
                connect_targets(&verge(case.verge.0, case.verge.1), &item),
                case.targets,
                "{label}"
            );
            assert_eq!(locked_targets(&item), case.locked, "{label}");
        }
    }

    #[test]
    fn unknown_provider_wording_is_no_header_at_all() {
        assert_eq!(provider_targets("tunnel"), None);
        assert_eq!(provider_targets(""), None);
    }
}
