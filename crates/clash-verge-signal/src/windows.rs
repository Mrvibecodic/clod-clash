use clash_verge_logging::{Type, logging};
use tokio::signal::windows;

use crate::Shutdown;

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

        logging!(info, Type::SystemSignal, "Caught Windows signal: {}", signal_name);

        f(pace).await;
    }
}
