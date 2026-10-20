mod backup;
mod clash;
mod config;
// clod: подключение — действие пользователя; что поднято при старте, решает
// «Подключаться при запуске», а не пережившие перезапуск флаги.
mod connect;
// clod:wake-net — сон машины и смена сети видны одному сторожу.
pub mod environment;
mod icon;
mod profile;
mod proxy;
mod proxy_chain;
mod session;
// clod:tun-ready — TUN живёт отдельным модулем: желание, заявка и факт.
pub mod tun;
mod window;

// Re-export all functions from modules
pub use backup::*;
pub use clash::*;
pub use config::*;
pub use connect::*;
pub use icon::*;
pub use profile::*;
pub use proxy::*;
pub use proxy_chain::*;
pub use session::*;
pub use window::*;
