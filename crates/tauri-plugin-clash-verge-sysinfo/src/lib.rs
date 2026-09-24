#[cfg(windows)]
use deelevate::{PrivilegeLevel, Token};
#[cfg(unix)]
pub use libc;
use sysinfo::Networks;
use tauri::{
    Manager as _, Runtime,
    plugin::{Builder, TauriPlugin},
};

struct AppIsAdmin(bool);

#[inline]
fn is_binary_admin() -> bool {
    #[cfg(not(windows))]
    unsafe {
        libc::geteuid() == 0
    }
    #[cfg(windows)]
    Token::with_current_process()
        .and_then(|token| token.privilege_level())
        .map(|level| level != PrivilegeLevel::NotPrivileged)
        .unwrap_or(false)
}

#[inline]
#[cfg(unix)]
pub fn current_gid() -> u32 {
    unsafe { libc::getgid() }
}

#[inline]
pub fn list_network_interfaces() -> Vec<String> {
    let mut networks = Networks::new();
    networks.refresh(false);
    networks.keys().map(|name| name.to_owned()).collect()
}

#[inline]
pub fn is_current_app_handle_admin<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.state::<AppIsAdmin>().0
}

#[inline]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::<R>::new("clash_verge_sysinfo")
        .setup(move |app, _api| {
            app.manage(AppIsAdmin(is_binary_admin()));
            Ok(())
        })
        .build()
}
