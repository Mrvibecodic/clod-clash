use super::resolve;
use crate::{
    config::{Config, DEFAULT_PAC, IVerge},
    module::lightweight,
    process::AsyncHandler,
    utils::window_manager::WindowManager,
};
use anyhow::{Result, bail};
use clash_verge_logging::{Type, logging, logging_error};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use reqwest::ClientBuilder;
use smartstring::alias::String;
use std::time::Duration;
use tokio::sync::oneshot;
use warp::Filter as _;

#[derive(serde::Deserialize, Debug)]
struct QueryParam {
    param: String,
}

static SHUTDOWN_SENDER: OnceCell<Mutex<Option<oneshot::Sender<()>>>> = OnceCell::new();

/// Порт одиночного экземпляра держится с самой проверки: слушатель, занявший
/// его, доживает здесь до запуска встроенного сервера.
static CLAIMED_LISTENER: Mutex<Option<std::net::TcpListener>> = Mutex::new(None);

/// Единственность экземпляра — это замок на файле: он не зависит от того, дала
/// ли система порт, и снимается вместе с процессом. Порт — только канал, по
/// которому вторая копия передаёт первой команду.
static INSTANCE_LOCK: Mutex<Option<std::fs::File>> = Mutex::new(None);
const INSTANCE_LOCK_FILE: &str = "instance.lock";

/// Первый экземпляр занимает порт сразу, а отвечать начинает только после
/// инициализации: второй ждёт его ответа, а не считает порт чужим.
const HANDOVER_WAIT: Duration = Duration::from_secs(20);
const LOCK_RETRY: Duration = Duration::from_millis(100);
fn held_by_someone(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::AddrInUse
}

#[derive(Debug)]
pub struct AnotherInstanceRunning;

impl std::fmt::Display for AnotherInstanceRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("another instance is already running; the command was handed over")
    }
}

impl std::error::Error for AnotherInstanceRunning {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceLock {
    Ours,
    Theirs,
    /// Замок взять не удалось по причине, не связанной с другой копией:
    /// единственность тогда, как и прежде, судится по порту.
    Unavailable,
}

fn lock_the_file(file: &std::fs::File) -> InstanceLock {
    match file.try_lock() {
        Ok(()) => InstanceLock::Ours,
        Err(std::fs::TryLockError::WouldBlock) => InstanceLock::Theirs,
        Err(std::fs::TryLockError::Error(_)) => InstanceLock::Unavailable,
    }
}

fn take_the_instance_lock() -> InstanceLock {
    let file = crate::utils::dirs::preinit_app_home_dir().and_then(|home| {
        std::fs::create_dir_all(&home)?;
        Ok(std::fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(home.join(INSTANCE_LOCK_FILE))?)
    });
    let Ok(file) = file else {
        return InstanceLock::Unavailable;
    };
    let lock = lock_the_file(&file);
    if lock == InstanceLock::Ours {
        *INSTANCE_LOCK.lock() = Some(file);
    }
    lock
}

enum Handover {
    Delivered,
    Failed(anyhow::Error),
}

/// Наша ли это ссылка.
///
/// clod:A1-06 — ссылку ищем среди всех аргументов запуска, а без неё просим
/// показать окно: раньше смотрели только первый аргумент, и любой другой
/// (ярлык с ключом, файл по ассоциации) молча завершал вторую копию без показа
/// окна. На macOS ссылки приходят событием системы, а не аргументом, поэтому
/// там вторая копия всегда просит показать окно.
#[cfg(not(target_os = "macos"))]
fn is_scheme_link(arg: &str) -> bool {
    arg.starts_with("clash:") || arg.starts_with("clash-verge:") || arg.starts_with("clodclash:")
}

async fn hand_the_command_over(port: u16, wait: Duration) -> Handover {
    // Сосед на этой же машине: системный прокси между нами ни к чему.
    let client = match ClientBuilder::new().no_proxy().timeout(wait).build() {
        Ok(client) => client,
        Err(error) => return Handover::Failed(error.into()),
    };
    #[cfg(not(target_os = "macos"))]
    {
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

        // Итератор аргументов не `Send`: ссылку достаём до первого `.await`.
        let link = std::env::args().skip(1).find(|arg| is_scheme_link(arg));
        if let Some(param) = link {
            let encoded = utf8_percent_encode(&param, NON_ALPHANUMERIC);
            return match client
                .get(format!("http://127.0.0.1:{port}/commands/scheme?param={encoded}"))
                .send()
                .await
            {
                Ok(_) => Handover::Delivered,
                Err(error) => Handover::Failed(error.into()),
            };
        }
    }
    match client
        .get(format!("http://127.0.0.1:{port}/commands/visible"))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => Handover::Delivered,
        Ok(response) => Handover::Failed(anyhow::anyhow!("ответ {}", response.status())),
        Err(error) => Handover::Failed(error.into()),
    }
}

fn leave_to_the_running_copy() -> Result<()> {
    logging!(info, Type::Window, "another instance is already running, exiting");
    Err(AnotherInstanceRunning.into())
}

pub async fn check_singleton() -> Result<()> {
    let port = IVerge::get_singleton_port();
    let started = std::time::Instant::now();
    loop {
        let lock = take_the_instance_lock();
        if lock != InstanceLock::Theirs {
            return claim_the_port(port, lock).await;
        }
        // Стартующая копия порт уже держит, и запрос дожидается её ответа;
        // быстрым отказ бывает, когда порт закрыт, — тогда замок пробуем снова.
        match hand_the_command_over(port, HANDOVER_WAIT).await {
            Handover::Delivered => return leave_to_the_running_copy(),
            Handover::Failed(error) if started.elapsed() >= HANDOVER_WAIT => {
                bail!("another copy holds the instance lock and did not answer the command: {error}");
            }
            Handover::Failed(_) => tokio::time::sleep(LOCK_RETRY).await,
        }
    }
}

/// Замок наш (или его не у кого спросить): остаётся занять порт.
async fn claim_the_port(port: u16, lock: InstanceLock) -> Result<()> {
    let refusal = match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => {
            *CLAIMED_LISTENER.lock() = Some(listener);
            return Ok(());
        }
        Err(refusal) => refusal,
    };
    // Порт зарезервирован системой или на него нет прав — не повод не
    // запускаться: работаем без встроенного сервера.
    if !held_by_someone(&refusal) {
        return Ok(());
    }
    // Порт держит копия без замка — прежняя версия или другая установка.
    let error = match hand_the_command_over(port, HANDOVER_WAIT).await {
        Handover::Delivered => return leave_to_the_running_copy(),
        Handover::Failed(error) => error,
    };
    logging!(
        error,
        Type::Window,
        "порт {} занят, а на команду никто не ответил ({}): передать её некому",
        port,
        error
    );
    if lock == InstanceLock::Ours {
        // Замок доказывает, что другой нашей копии нет: порт занял посторонний.
        return Ok(());
    }
    bail!("singleton port {port} is held and nobody answered the command: {error}");
}

pub fn embed_server() {
    let Some(listener) = CLAIMED_LISTENER.lock().take() else {
        logging!(
            error,
            Type::Window,
            "порт одиночного экземпляра занять не удалось: встроенный сервер не поднят, второй запуск приложения не будет замечен"
        );
        return;
    };
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    #[allow(clippy::expect_used)]
    SHUTDOWN_SENDER
        .set(Mutex::new(Some(shutdown_tx)))
        .expect("failed to set shutdown signal for embedded server");

    // Показ окна не привязан к соединению: вторая копия вправе оборвать его
    // раньше, чем окно построится, а брошенный на полпути показ оставляет
    // лёгкий режим в промежуточном состоянии.
    let visible = warp::path!("commands" / "visible").and_then(|| async {
        logging!(
            info,
            Type::Window,
            "Обнаружено восстановление окна приложения из режима одиночного экземпляра"
        );
        AsyncHandler::spawn(|| async {
            if !lightweight::exit_lightweight_mode().await {
                WindowManager::show_main_window().await;
            }
        });
        Ok::<_, warp::Rejection>(warp::reply::with_status::<std::string::String>(
            "ok".to_string(),
            warp::http::StatusCode::OK,
        ))
    });

    let pac = warp::path!("commands" / "pac").and_then(|| async move {
        // clod:port-ladder — порт для PAC берётся из собранного конфига: при
        // «как в подписке» наши настройки его не знают.
        let pac_port = Config::effective_mixed_port().await;
        let verge_config = Config::verge().await;

        let verge_data = verge_config.data_arc();

        let pac_content = verge_data.pac_file_content.as_deref().unwrap_or(DEFAULT_PAC);

        let proxy_host = verge_data.proxy_host.as_deref().unwrap_or("127.0.0.1");
        let processed_content = pac_content
            .replace("%mixed-port%", &format!("{pac_port}"))
            .replace("%proxy_host%", proxy_host);
        Ok::<_, warp::Rejection>(
            warp::http::Response::builder()
                .header("Content-Type", "application/x-ns-proxy-autoconfig")
                .body(processed_content)
                .unwrap_or_default(),
        )
    });

    let scheme = warp::path!("commands" / "scheme")
        .and(warp::query::<QueryParam>())
        .and_then(|query: QueryParam| async move {
            AsyncHandler::spawn(|| async move {
                if !lightweight::exit_lightweight_mode().await {
                    WindowManager::show_main_window().await;
                }
                logging_error!(Type::Setup, resolve::resolve_scheme(&query.param).await);
            });
            Ok::<_, warp::Rejection>(warp::reply::with_status::<std::string::String>(
                "ok".to_string(),
                warp::http::StatusCode::OK,
            ))
        });

    let commands = visible.or(scheme).or(pac);

    AsyncHandler::spawn(move || async move {
        let listener = listener
            .set_nonblocking(true)
            .and_then(|()| tokio::net::TcpListener::from_std(listener));
        let listener = match listener {
            Ok(listener) => listener,
            Err(error) => {
                logging!(error, Type::Window, "встроенный сервер не поднят: {error}");
                return;
            }
        };
        warp::serve(commands)
            .incoming(listener)
            .graceful(async {
                shutdown_rx.await.ok();
            })
            .run()
            .await;
    });
}

pub fn shutdown_embedded_server() {
    logging!(info, Type::Window, "shutting down embedded server");
    // Вместе с сервером отпускается и замок экземпляра: копия, которую мы
    // породим при перезапуске, не должна ждать нашей смерти.
    INSTANCE_LOCK.lock().take();
    if let Some(sender) = SHUTDOWN_SENDER.get()
        && let Some(sender) = sender.lock().take()
    {
        sender.send(()).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::{InstanceLock, held_by_someone, lock_the_file};
    use std::net::{Ipv4Addr, TcpListener};
    use warp::Filter as _;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn only_our_schemes_are_links() {
        use super::is_scheme_link;
        for link in ["clodclash://install-config?url=x", "clash://a", "clash-verge://a"] {
            assert!(is_scheme_link(link), "{link}");
        }
        for other in ["--some-flag", "C:\\profile.yaml", "https://example.com", ""] {
            assert!(!is_scheme_link(other), "{other}");
        }
    }

    #[test]
    fn a_port_someone_listens_on_is_told_apart_from_a_port_the_system_refuses() {
        let first = TcpListener::bind((Ipv4Addr::LOCALHOST, 0));
        let port = first
            .as_ref()
            .ok()
            .and_then(|listener| listener.local_addr().ok())
            .map(|address| address.port())
            .unwrap_or_default();
        assert_ne!(port, 0);

        let second = TcpListener::bind((Ipv4Addr::LOCALHOST, port));
        assert!(second.is_err_and(|error| held_by_someone(&error)));

        let refused = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(!held_by_someone(&refused));
    }

    #[test]
    fn the_instance_lock_is_held_by_one_copy_until_it_lets_go() {
        let path = std::env::temp_dir().join(format!("clod-instance-{}.lock", std::process::id()));
        let open = || {
            std::fs::File::options()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
        };
        let (Ok(first), Ok(second)) = (open(), open()) else {
            return assert!(path.exists(), "файл замка не открылся");
        };

        assert_eq!(lock_the_file(&first), InstanceLock::Ours);
        assert_eq!(lock_the_file(&second), InstanceLock::Theirs);
        drop(first);
        assert_eq!(lock_the_file(&second), InstanceLock::Ours);

        drop(second);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_command_sent_before_the_server_starts_is_answered_once_it_does() {
        let claimed = TcpListener::bind((Ipv4Addr::LOCALHOST, 0));
        let port = claimed
            .as_ref()
            .ok()
            .and_then(|listener| listener.local_addr().ok())
            .map(|address| address.port())
            .unwrap_or_default();
        assert_ne!(port, 0);

        let asking = tokio::spawn(async move {
            let client = reqwest::ClientBuilder::new()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .ok()?;
            client.get(format!("http://127.0.0.1:{port}/")).send().await.ok()
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let listener = claimed
            .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
            .and_then(tokio::net::TcpListener::from_std);
        assert!(listener.is_ok());
        if let Ok(listener) = listener {
            tokio::spawn(
                warp::serve(warp::any().map(|| "ok".to_owned()))
                    .incoming(listener)
                    .run(),
            );
        }

        let answer = asking.await.ok().flatten();
        assert!(answer.is_some_and(|response| response.status().is_success()));
    }
}
