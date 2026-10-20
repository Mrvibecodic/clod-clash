use std::sync::atomic::{AtomicBool, Ordering};

use clash_verge_logging::{Type, logging};
use tokio::signal::windows;

use crate::Shutdown;

static IS_CLEANING_UP: AtomicBool = AtomicBool::new(false);

pub async fn listen<F, Fut>(f: F)
where
    F: Fn(Shutdown) -> Fut + Send + Sync + 'static,
    Fut: Future + Send + 'static,
{
    let mut ctrl_c = match windows::ctrl_c() {
        Ok(s) => s,
        Err(e) => {
            logging!(error, Type::SystemSignal, "Failed to register Ctrl+C: {}", e);
            return;
        }
    };

    let mut ctrl_close = match windows::ctrl_close() {
        Ok(s) => s,
        Err(e) => {
            logging!(error, Type::SystemSignal, "Failed to register Ctrl+Close: {}", e);
            return;
        }
    };

    loop {
        let signal_name;
        let pace;
        tokio::select! {
            _ = ctrl_c.recv() => {
                signal_name = "Ctrl+C";
                pace = Shutdown::Interactive;
            }
            _ = ctrl_close.recv() => {
                signal_name = "Ctrl+Close";
                pace = Shutdown::SessionEnding;
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

        logging!(info, Type::SystemSignal, "Caught Windows signal: {}", signal_name);

        f(pace).await;
    }
}
