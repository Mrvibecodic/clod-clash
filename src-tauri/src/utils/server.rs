use super::resolve;
use crate::{
    cmd::network::port_is_taken_at,
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

pub async fn check_singleton() -> Result<()> {
    let port = IVerge::get_singleton_port();
    if port_is_taken_at(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), port) {
        let client = ClientBuilder::new().timeout(Duration::from_millis(500)).build()?;
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
                "порт {} занят посторонним процессом ({}): передать команду некому",
                port,
                error
            );
            bail!("singleton port {port} is held by a foreign process: {error}");
        }
        logging!(
            info,
            Type::Window,
            "another instance is already running; the command was handed over, exiting"
        );
        bail!("app exists");
    }
    Ok(())
}

pub fn embed_server() {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    #[allow(clippy::expect_used)]
    SHUTDOWN_SENDER
        .set(Mutex::new(Some(shutdown_tx)))
        .expect("failed to set shutdown signal for embedded server");
    let port = IVerge::get_singleton_port();

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
        let verge_config = Config::verge().await;
        let clash_config = Config::clash().await;

        let verge_data = verge_config.data_arc();
        let clash_data = clash_config.data_arc();

        let pac_content = verge_data.pac_file_content.as_deref().unwrap_or(DEFAULT_PAC);

        let pac_port = verge_data
            .verge_mixed_port
            .unwrap_or_else(|| clash_data.get_mixed_port());
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
        warp::serve(commands)
            .bind(([127, 0, 0, 1], port))
            .await
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
