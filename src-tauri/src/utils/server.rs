use super::resolve;
use crate::{
    config::{Config, DEFAULT_PAC, IVerge, LOOPBACK_PROXY_HOST},
    module::lightweight,
    process::AsyncHandler,
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

static SHUTDOWN_SENDER: OnceCell<Mutex<Option<oneshot::Sender<()>>>> = OnceCell::new();

/// Порт одиночного экземпляра держится с самой проверки: слушатель, занявший
/// его, доживает здесь до запуска встроенного сервера.
static CLAIMED_LISTENER: Mutex<Option<std::net::TcpListener>> = Mutex::new(None);

/// Единственность экземпляра — это замок на файле: он не зависит от того, дала
/// ли система порт, и снимается вместе с процессом. Порт — только канал, по
/// которому вторая копия передаёт первой команду.
static INSTANCE_LOCK: Mutex<Option<std::fs::File>> = Mutex::new(None);
const INSTANCE_LOCK_FILE: &str = "instance.lock";

/// Ссылка не ходит через порт: команду «установи подписку» принял бы любой,
/// кто до порта дотянулся, включая страницу в браузере. Вторая копия кладёт
/// ссылку сюда — в каталог, куда пишет только наш пользователь, — и просит
/// показать окно обычной командой.
const PENDING_LINK_FILE: &str = "pending-link";

/// Команды порта принимаются только от того, кто прочёл токен экземпляра из
/// файла, доступного одному нашему пользователю: страница в браузере до порта
/// дотянется, а до файла — нет. PAC остаётся открытым — его берёт браузер.
const INSTANCE_TOKEN_FILE: &str = "instance-token";
const INSTANCE_TOKEN_HEADER: &str = "x-instance-token";
static INSTANCE_TOKEN: OnceCell<std::string::String> = OnceCell::new();

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
fn is_scheme_link(arg: &str) -> bool {
    arg.split_once(':')
        .is_some_and(|(scheme, _)| crate::constants::branding::DEEP_LINK_SCHEMES.contains(&scheme))
}

/// Командная строка для следующей копии этого процесса (перезапуск,
/// установщик обновления) — без ссылок: их уже разобрала эта копия, и
/// следующая добавила бы удалённую с тех пор подписку снова.
pub fn without_links(args: impl IntoIterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    args.into_iter()
        .filter(|arg| !arg.to_str().is_some_and(is_scheme_link))
        .collect()
}

/// Каждая ссылка кладётся своим файлом: две подряд не затирают друг друга, а
/// забирающий получает её только вместе с правом унести — переименованием.
async fn leave_the_link_for_the_running_copy(link: &str) -> Result<std::path::PathBuf> {
    let path = crate::utils::dirs::preinit_app_home_dir()?
        .join(format!("{PENDING_LINK_FILE}{}", crate::utils::help::get_uid("-")));
    crate::utils::help::write_atomic(&path, link.as_bytes()).await?;
    Ok(path)
}

async fn claim_the_link_at(path: &std::path::Path) -> Option<String> {
    // Имя забранного файла подметает та же уборка, что и прочие недописанные:
    // копия, умершая между «забрал» и «прочитал», мусора не оставит.
    let claimed = crate::utils::help::staging_path(path);
    tokio::fs::rename(path, &claimed).await.ok()?;
    let link = tokio::fs::read_to_string(&claimed).await;
    let _ = tokio::fs::remove_file(&claimed).await;
    is_scheme_link(link.as_deref().unwrap_or_default().trim()).then(|| link.unwrap_or_default().trim().into())
}

/// Забрать ссылки, оставленные вторыми копиями, и стереть их след.
pub async fn take_the_pending_links() -> Vec<String> {
    let Ok(home) = crate::utils::dirs::preinit_app_home_dir() else {
        return Vec::new();
    };
    let Ok(mut entries) = tokio::fs::read_dir(&home).await else {
        return Vec::new();
    };
    let mut pending = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if !entry.file_name().to_string_lossy().starts_with(PENDING_LINK_FILE) {
            continue;
        }
        if let Some(link) = claim_the_link_at(&entry.path()).await {
            pending.push(link);
        }
    }
    pending
}

fn instance_token_path() -> Result<std::path::PathBuf> {
    Ok(crate::utils::dirs::preinit_app_home_dir()?.join(INSTANCE_TOKEN_FILE))
}

/// Новый токен на каждый запуск, в файл, закрытый от других пользователей.
fn issue_the_instance_token() -> Result<std::string::String> {
    use std::io::Write as _;

    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("no randomness for the instance token: {error}"))?;
    let token = hex::encode(bytes);
    let path = instance_token_path()?;
    let _ = std::fs::remove_file(&path);
    #[cfg(windows)]
    let mut file = crate::core::owner_identity::create_private_current_user_file(&path)?;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?
    };
    file.write_all(token.as_bytes())?;
    Ok(token)
}

/// Токен работающей копии; пока она его не записала — нет, и вторая копия
/// спросит ещё раз.
fn read_the_instance_token() -> Option<std::string::String> {
    use std::io::Read as _;

    let path = instance_token_path().ok()?;
    #[cfg(windows)]
    let mut file = crate::core::owner_identity::open_private_current_user_file(&path).ok()?;
    #[cfg(unix)]
    let mut file = std::fs::File::open(&path).ok()?;
    let mut token = std::string::String::new();
    file.read_to_string(&mut token).ok()?;
    Some(token.trim().to_owned()).filter(|token| !token.is_empty())
}

fn presents_the_token(presented: Option<&str>, issued: Option<&str>) -> bool {
    match (presented, issued) {
        (Some(presented), Some(issued)) => {
            presented.len() == issued.len()
                && presented
                    .bytes()
                    .zip(issued.bytes())
                    .fold(0_u8, |differs, (a, b)| differs | (a ^ b))
                    == 0
        }
        _ => false,
    }
}

async fn ask_to_show_the_window(port: u16, wait: Duration) -> Handover {
    // Сосед на этой же машине: системный прокси между нами ни к чему.
    let client = match ClientBuilder::new().no_proxy().timeout(wait).build() {
        Ok(client) => client,
        Err(error) => return Handover::Failed(error.into()),
    };
    let mut request = client.get(format!("http://127.0.0.1:{port}/commands/visible"));
    if let Some(token) = read_the_instance_token() {
        request = request.header(INSTANCE_TOKEN_HEADER, token);
    }
    match request.send().await {
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
    let mut left_at = None;
    let outcome = look_for_the_running_copy(IVerge::get_singleton_port(), &mut left_at).await;
    // Ссылку уносит только копия, уступившая работающей: если мы остаёмся
    // работать или уходим с ошибкой, ссылку разберёт наш собственный запуск.
    if !outcome
        .as_ref()
        .is_err_and(|error| error.is::<AnotherInstanceRunning>())
        && let Some(path) = left_at
    {
        let _ = tokio::fs::remove_file(path).await;
    }
    outcome
}

/// Оставить ссылку из аргументов запуска — один раз за попытку передачи.
async fn leave_the_link_once(left_at: &mut Option<std::path::PathBuf>) {
    if left_at.is_some() {
        return;
    }
    // Итератор аргументов не `Send`: ссылку достаём до первого `.await`.
    let Some(link) = std::env::args().skip(1).find(|arg| is_scheme_link(arg)) else {
        return;
    };
    match leave_the_link_for_the_running_copy(&link).await {
        Ok(path) => *left_at = Some(path),
        Err(error) => logging!(error, Type::Window, "ссылку передать не удалось: {error}"),
    }
}

async fn look_for_the_running_copy(port: u16, left_at: &mut Option<std::path::PathBuf>) -> Result<()> {
    let started = std::time::Instant::now();
    loop {
        let lock = take_the_instance_lock();
        if lock != InstanceLock::Theirs {
            return claim_the_port(port, lock, left_at).await;
        }
        leave_the_link_once(left_at).await;
        // Стартующая копия порт уже держит, и запрос дожидается её ответа;
        // быстрым отказ бывает, когда порт закрыт, токен ещё не записан или
        // копия уходит, — тогда замок пробуем снова.
        match ask_to_show_the_window(port, HANDOVER_WAIT).await {
            Handover::Delivered => return leave_to_the_running_copy(),
            Handover::Failed(error) if started.elapsed() >= HANDOVER_WAIT => {
                bail!("another copy holds the instance lock and did not answer the command: {error}");
            }
            Handover::Failed(_) => tokio::time::sleep(LOCK_RETRY).await,
        }
    }
}

/// Замок наш (или его не у кого спросить): остаётся занять порт.
async fn claim_the_port(port: u16, lock: InstanceLock, left_at: &mut Option<std::path::PathBuf>) -> Result<()> {
    let refusal = match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => {
            match issue_the_instance_token() {
                Ok(token) => {
                    let _ = INSTANCE_TOKEN.set(token);
                }
                Err(error) => logging!(
                    error,
                    Type::Window,
                    "токен экземпляра не записан ({error:#}): второй запуск окно не покажет"
                ),
            }
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
    leave_the_link_once(left_at).await;
    let error = match ask_to_show_the_window(port, HANDOVER_WAIT).await {
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
    // Пока идёт выход, команду не берём: вторая копия подождёт и встанет
    // сама, а ссылка дождётся её, а не уходящей копии.
    let visible = warp::path!("commands" / "visible")
        .and(warp::header::optional::<std::string::String>(INSTANCE_TOKEN_HEADER))
        .and_then(|presented: Option<std::string::String>| async move {
            let status = if !presents_the_token(
                presented.as_deref(),
                INSTANCE_TOKEN.get().map(std::string::String::as_str),
            ) {
                logging!(
                    warn,
                    Type::Window,
                    "команда показа окна без токена экземпляра отклонена"
                );
                warp::http::StatusCode::FORBIDDEN
            } else if crate::core::handle::Handle::global().is_exiting() {
                logging!(info, Type::Window, "команда показа окна отклонена: идёт выход");
                warp::http::StatusCode::SERVICE_UNAVAILABLE
            } else {
                logging!(
                    info,
                    Type::Window,
                    "Обнаружено восстановление окна приложения из режима одиночного экземпляра"
                );
                AsyncHandler::spawn(|| async {
                    lightweight::wake_or_show().await;
                    for link in take_the_pending_links().await {
                        logging_error!(Type::Setup, resolve::resolve_scheme(&link).await);
                    }
                });
                warp::http::StatusCode::OK
            };
            Ok::<_, warp::Rejection>(warp::reply::with_status::<std::string::String>(
                status.canonical_reason().unwrap_or_default().to_owned(),
                status,
            ))
        });

    let pac = warp::path!("commands" / "pac").and_then(|| async move {
        // clod:port-ladder — порт для PAC берётся из собранного конфига: при
        // «как в подписке» наши настройки его не знают.
        let pac_port = Config::effective_mixed_port().await;
        let verge_config = Config::verge().await;

        let verge_data = verge_config.data_arc();

        let pac_content = verge_data.pac_file_content.as_deref().unwrap_or(DEFAULT_PAC);

        let processed_content = pac_content
            .replace("%mixed-port%", &format!("{pac_port}"))
            .replace("%proxy_host%", LOOPBACK_PROXY_HOST);
        Ok::<_, warp::Rejection>(
            warp::http::Response::builder()
                .header("Content-Type", "application/x-ns-proxy-autoconfig")
                .body(processed_content)
                .unwrap_or_default(),
        )
    });

    let commands = visible.or(pac);

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
        INSTANCE_LOCK.lock().take();
    });
}

pub fn shutdown_embedded_server() {
    logging!(info, Type::Window, "shutting down embedded server");
    // Замок экземпляра отпускается, когда порт закрыт, но не позже: копия,
    // взявшая замок раньше, нашла бы порт занятым и осталась бы без сервера.
    // Копия, которую мы породим при перезапуске, нашей смерти не ждёт.
    let Some(sender) = SHUTDOWN_SENDER.get() else {
        INSTANCE_LOCK.lock().take();
        return;
    };
    if let Some(sender) = sender.lock().take()
        && sender.send(()).is_err()
    {
        INSTANCE_LOCK.lock().take();
    }
}

#[cfg(test)]
mod tests {
    use super::{InstanceLock, held_by_someone, lock_the_file};
    use std::net::{Ipv4Addr, TcpListener};
    use warp::Filter as _;

    /// Замок экземпляра уходит, когда порт уже закрыт: копия, взявшая его,
    /// всегда находит порт свободным.
    #[test]
    fn the_instance_lock_is_let_go_only_once_the_port_is_closed() {
        use crate::utils::source_scan::fn_body;
        let source = include_str!("server.rs");
        let server = fn_body(source, "pub fn embed_server(").unwrap_or_default();
        let served = server.find(".run()").unwrap_or(usize::MAX);
        let released = server.find("INSTANCE_LOCK.lock().take()").unwrap_or_default();
        assert!(released > served, "{server}");
        let shutdown = fn_body(source, "pub fn shutdown_embedded_server(").unwrap_or_default();
        let signalled = shutdown.find("sender.send(())").unwrap_or(usize::MAX);
        assert!(
            shutdown.matches("INSTANCE_LOCK.lock().take()").count() == 2
                && shutdown.rfind("INSTANCE_LOCK.lock().take()").unwrap_or_default() > signalled,
            "{shutdown}"
        );
    }

    #[test]
    fn only_the_issued_instance_token_opens_the_commands() {
        use super::presents_the_token;
        let issued = "0123456789abcdef";
        assert!(presents_the_token(Some(issued), Some(issued)));
        for presented in [None, Some(""), Some("0123456789abcdee"), Some("0123456789abcdef0")] {
            assert!(!presents_the_token(presented, Some(issued)), "{presented:?}");
        }
        assert!(!presents_the_token(Some(issued), None));
        assert!(!presents_the_token(None, None));
    }

    #[test]
    fn only_our_schemes_are_links() {
        use super::is_scheme_link;
        for link in ["clodclash://install-config?url=x", "clash://a"] {
            assert!(is_scheme_link(link), "{link}");
        }
        for other in [
            "--some-flag",
            "C:\\profile.yaml",
            "https://example.com",
            "clash-verge://a",
            "",
        ] {
            assert!(!is_scheme_link(other), "{other}");
        }
    }

    #[test]
    fn the_next_copy_does_not_get_the_links_again() {
        use std::ffi::OsString;
        let args = [
            "C:\\Program Files\\Clod Clash\\clod-clash.exe",
            "clodclash://install-config?url=https%3A%2F%2Fsub.example.com%2Fa",
            "--minimized",
            "clash://install-config?url=https://sub.example.com/b",
        ]
        .map(OsString::from);
        assert_eq!(
            super::without_links(args),
            ["C:\\Program Files\\Clod Clash\\clod-clash.exe", "--minimized"].map(OsString::from)
        );
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
    async fn a_left_link_is_claimed_once_and_leaves_no_trace() {
        let path = std::env::temp_dir().join(format!("clod-pending-{}", std::process::id()));
        let link = "clodclash://install-config?url=https://example.com/sub";
        assert!(std::fs::write(&path, link).is_ok());

        assert_eq!(super::claim_the_link_at(&path).await.as_deref(), Some(link));
        assert!(super::claim_the_link_at(&path).await.is_none());
        assert!(!path.exists());
        assert!(!path.with_extension("taken").exists());
    }

    #[tokio::test]
    async fn a_file_that_holds_no_link_of_ours_is_claimed_and_dropped() {
        let path = std::env::temp_dir().join(format!("clod-pending-junk-{}", std::process::id()));
        assert!(std::fs::write(&path, "https://example.com/sub").is_ok());

        assert!(super::claim_the_link_at(&path).await.is_none());
        assert!(!path.exists());
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
