use std::sync::atomic::{AtomicBool, Ordering};

use clash_verge_logging::{Type, logging};
use tokio::signal::unix::{SignalKind, signal};

use crate::{RUNTIME, Shutdown};

static IS_CLEANING_UP: AtomicBool = AtomicBool::new(false);

pub fn register<F, Fut>(f: F)
where
    F: Fn(Shutdown) -> Fut + Send + Sync + 'static,
    Fut: Future + Send + 'static,
{
    if let Some(Some(rt)) = RUNTIME.get() {
        rt.spawn(async move {
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

                if IS_CLEANING_UP.load(Ordering::SeqCst) {
                    logging!(
                        info,
                        Type::SystemSignal,
                        "Already shutting down, ignoring repeated signal: {}",
                        signal_name
                    );
                    continue;
                }
                IS_CLEANING_UP.store(true, Ordering::SeqCst);

                logging!(info, Type::SystemSignal, "Caught signal {}", signal_name);

                f(pace).await;
            }
        });
    } else {
        logging!(
            error,
            Type::SystemSignal,
            "register shutdown signal failed, RUNTIME is not available"
        );
    }
}
