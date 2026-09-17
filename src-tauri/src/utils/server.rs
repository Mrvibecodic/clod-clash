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

/// Первый экземпляр занимает порт сразу, а отвечать начинает только после
/// инициализации: второй ждёт его ответа, а не считает порт чужим.
const HANDOVER_WAIT: Duration = Duration::from_secs(20);

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

pub async fn check_singleton() -> Result<()> {
    let port = IVerge::get_singleton_port();
    let claim = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port));
    let held = claim.as_ref().is_err_and(held_by_someone);
    // Отказ не про занятый порт (порт зарезервирован системой, нет прав) —
    // не повод не запускаться: работаем без встроенного сервера.
    *CLAIMED_LISTENER.lock() = claim.ok();
    if held {
        // Сосед на этой же машине: системный прокси между нами ни к чему.
        let client = ClientBuilder::new().no_proxy().timeout(HANDOVER_WAIT).build()?;
        #[allow(clippy::needless_collect)]
        let argvs: Vec<std::string::String> = std::env::args().collect();
        let mut handover: Result<()> = Ok(());
        if argvs.len() > 1 {
            #[cfg(not(target_os = "macos"))]
            {
                use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

                let param = argvs[1].as_str();
                if param.starts_with("clash:") || param.starts_with("clash-verge:") || param.starts_with("clodclash:") {
                    let encoded = utf8_percent_encode(param, NON_ALPHANUMERIC);
                    handover = client
                        .get(format!("http://127.0.0.1:{port}/commands/scheme?param={encoded}"))
                        .send()
                        .await
                        .map(|_| ())
                        .map_err(anyhow::Error::from);
                }
            }
        } else {
            handover = client
                .get(format!("http://127.0.0.1:{port}/commands/visible"))
                .send()
                .await
                .map_err(anyhow::Error::from)
                .and_then(|response| {
                    let status = response.status();
                    if status.is_success() {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!("ответ {status}"))
                    }
                });
        }
        if let Err(error) = handover {
            logging!(
                error,
                Type::Window,
                "порт {} занят, а на команду никто не ответил ({}): передать её некому",
                port,
                error
            );
            bail!("singleton port {port} is held and nobody answered the command: {error}");
        }
        logging!(
            info,
            Type::Window,
            "another instance is already running; the command was handed over, exiting"
        );
        return Err(AnotherInstanceRunning.into());
    }
    Ok(())
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

    let visible = warp::path!("commands" / "visible").and_then(|| async {
        logging!(
            info,
            Type::Window,
            "Обнаружено восстановление окна приложения из режима одиночного экземпляра"
        );
        if !lightweight::exit_lightweight_mode().await {
            WindowManager::show_main_window().await;
        }
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
    if let Some(sender) = SHUTDOWN_SENDER.get()
        && let Some(sender) = sender.lock().take()
    {
        sender.send(()).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::held_by_someone;
    use std::net::{Ipv4Addr, TcpListener};
    use warp::Filter as _;

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
