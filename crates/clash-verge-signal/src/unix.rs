use clash_verge_logging::{Type, logging};
use tokio::signal::unix::{SignalKind, signal};

use crate::Shutdown;

pub async fn listen<F, Fut>(f: F)
where
    F: Fn(Shutdown) -> Fut + Send + Sync + 'static,
    Fut: Future + Send + 'static,
{
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            logging!(error, Type::SystemSignal, "Failed to register SIGTERM: {}", e);
            return;
        }
    };
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(e) => {
            logging!(error, Type::SystemSignal, "Failed to register SIGINT: {}", e);
            return;
        }
    };
    let mut sighup = match signal(SignalKind::hangup()) {
        Ok(s) => s,
        Err(e) => {
            logging!(error, Type::SystemSignal, "Failed to register SIGHUP: {}", e);
            return;
        }
    };

    loop {
        let signal_name;
        let pace;
        tokio::select! {
            _ = sigterm.recv() => {
                signal_name = "SIGTERM";
                pace = Shutdown::SessionEnding;
            }
            _ = sigint.recv() => {
                signal_name = "SIGINT";
                pace = Shutdown::Interactive;
            }
            _ = sighup.recv() => {
                signal_name = "SIGHUP";
                pace = Shutdown::SessionEnding;
            }
            else => {
                break;
            }
        }

        logging!(info, Type::SystemSignal, "Caught signal {}", signal_name);

        f(pace).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::Shutdown;

    #[tokio::test(flavor = "current_thread")]
    async fn a_hangup_reaches_the_handler_on_the_callers_runtime() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(super::listen(move |pace| {
            let sender = sender.clone();
            async move {
                let _ = sender.send(pace);
            }
        }));
        tokio::task::yield_now().await;

        let killed = std::process::Command::new("kill")
            .arg("-HUP")
            .arg(std::process::id().to_string())
            .status()
            .map(|status| status.success());
        assert!(matches!(killed, Ok(true)), "{killed:?}");

        let pace = tokio::time::timeout(Duration::from_secs(5), receiver.recv()).await;
        assert!(matches!(pace, Ok(Some(Shutdown::SessionEnding))), "{pace:?}");
    }

    /// Повторный сигнал тоже доходит до приложения: решает, идёт ли уже
    /// выход, оно само, а не крейт по своему одноразовому признаку.
    #[tokio::test(flavor = "current_thread")]
    async fn a_second_signal_reaches_the_handler_too() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(super::listen(move |pace| {
            let sender = sender.clone();
            async move {
                let _ = sender.send(pace);
            }
        }));
        tokio::task::yield_now().await;

        for round in 1..=2 {
            let killed = std::process::Command::new("kill")
                .arg("-HUP")
                .arg(std::process::id().to_string())
                .status()
                .map(|status| status.success());
            assert!(matches!(killed, Ok(true)), "{killed:?}");
            let pace = tokio::time::timeout(Duration::from_secs(5), receiver.recv()).await;
            assert!(
                matches!(pace, Ok(Some(Shutdown::SessionEnding))),
                "сигнал {round}: {pace:?}"
            );
        }
    }
}
