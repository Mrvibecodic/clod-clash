use crate::utils::redact;
use anyhow::Result;
use smartstring::alias::String;

pub type CmdResult<T = ()> = Result<T, String>;

pub mod app;
pub mod backup;
pub mod clash;
pub mod core_updater;
pub mod lightweight;
pub mod media_unlock_checker;
pub mod network;
pub mod profile;
pub mod proxy;
pub mod runtime;
pub mod save_profile;
pub mod service;
pub mod system;
pub mod uwp;
pub mod validate;
pub mod verge;
pub mod webdav;

pub use app::*;
pub use backup::*;
pub use clash::*;
pub use core_updater::*;
pub use lightweight::*;
pub use media_unlock_checker::*;
pub use network::*;
pub use profile::*;
pub use proxy::*;
pub use runtime::*;
pub use save_profile::*;
pub use service::*;
pub use system::*;
pub use uwp::*;
pub use verge::*;
pub use webdav::*;

pub trait StringifyErr<T> {
    fn stringify_err(self) -> CmdResult<T>;
    fn stringify_err_log<F>(self, log_fn: F) -> CmdResult<T>
    where
        F: Fn(&str);
}

pub(crate) fn public_error_text(error: &impl std::fmt::Display) -> String {
    let raw = error.to_string();
    let home = redact::home_prefix();
    String::from(redact::redact(&redact::scrub_home(&raw, home.as_deref())))
}

impl<T, E: std::fmt::Display> StringifyErr<T> for Result<T, E> {
    fn stringify_err(self) -> CmdResult<T> {
        self.map_err(|e| public_error_text(&e))
    }

    fn stringify_err_log<F>(self, log_fn: F) -> CmdResult<T>
    where
        F: Fn(&str),
    {
        self.map_err(|e| {
            let msg = public_error_text(&e);
            log_fn(&msg);
            msg
        })
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::StringifyErr as _;

    #[test]
    fn error_text_for_the_frontend_is_redacted() {
        let err: Result<(), anyhow::Error> = Err(anyhow::anyhow!("token: abcdefghij0123456789XYZ failed"));
        let text = err.stringify_err().unwrap_err();
        assert!(!text.contains("abcdefghij0123456789XYZ"), "{text}");
        assert!(text.contains("failed"));
    }
}
