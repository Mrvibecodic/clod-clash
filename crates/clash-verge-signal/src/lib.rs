#[cfg(unix)]
mod unix;
#[cfg(all(windows, debug_assertions))]
mod windows;

/// Каким образом нас просят закрыться.
///
/// clod:exit-pace — Ctrl+C и подобное приходят от человека: он видит окно и
/// подождёт, пока мы честно снимем системный прокси. Выключение компьютера и
/// выход из сеанса дают считаные секунды, после которых нас убьют, поэтому там
/// уборка идёт по укороченным бюджетам и «как получится».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shutdown {
    Interactive,
    SessionEnding,
}

/// Оконная сборка Windows без консоли сигналов не получает: выход из сеанса и
/// выключение приходят через событие выхода приложения.
#[cfg_attr(all(windows, not(debug_assertions)), allow(clippy::unused_async))]
pub async fn listen<F, Fut>(f: F)
where
    F: Fn(Shutdown) -> Fut + Send + Sync + 'static,
    Fut: Future + Send + 'static,
{
    #[cfg(unix)]
    unix::listen(f).await;

    #[cfg(all(windows, debug_assertions))]
    windows::listen(f).await;

    #[cfg(all(windows, not(debug_assertions)))]
    let _ = f;
}
