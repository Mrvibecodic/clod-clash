use crate::config::Config;
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use reqwest::{
    Client, Proxy, StatusCode,
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use smartstring::alias::String;
use std::{sync::Arc, time::Duration};
use sysproxy::Sysproxy;
use tauri::Url;

#[derive(Debug)]
pub struct HttpResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl HttpResponse {
    pub const fn new(status: StatusCode, headers: HeaderMap, body: String) -> Self {
        Self { status, headers, body }
    }

    pub const fn status(&self) -> StatusCode {
        self.status
    }

    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn text_with_charset(&self) -> Result<&str> {
        Ok(&self.body)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProxyType {
    None,
    Localhost,
    System,
}

#[derive(Debug, Clone, Copy)]
enum TlsRootMode {
    PlatformVerifier,
    StaticWebpkiRoots,
}

const MAX_REDIRECTS: usize = 10;

/// Больше этого тело ответа не читается: подписка столько не весит.
const MAX_BODY: usize = 64 << 20;

pub(crate) const DOWNGRADE_REFUSED: &str = "redirect from https to http is refused";

/// Уводит ли редирект с закрытого адреса на открытый.
///
/// Смотрим на самый первый адрес цепочки, а не на предыдущий шаг: важно, что обещал
/// пользователю его собственный адрес подписки. Если он изначально был `http://`,
/// понижать нечего и цепочка идёт как раньше.
fn redirect_is_a_downgrade(first: Option<&Url>, next: &Url) -> bool {
    first.is_some_and(|first| first.scheme() == "https") && next.scheme() != "https"
}

/// Адрес системного прокси, если он в системе включён.
///
/// clod:Э9-07 — единственное место, которое читает системный прокси для
/// исходящих запросов: по нему же проверяется, отличается ли маршрут «через
/// системный прокси» от прямого. Читаем именно систему, а не свой тумблер:
/// прокси мог поставить кто угодно.
pub fn system_proxy_url() -> Option<std::string::String> {
    match Sysproxy::get_system_proxy() {
        Ok(p @ Sysproxy { enable: true, .. }) => Some(format!("http://{}:{}", p.host, p.port)),
        _ => None,
    }
}

pub struct NetworkManager;

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkManager {
    pub const fn new() -> Self {
        Self
    }

    fn build_client(
        &self,
        proxy_url: Option<std::string::String>,
        default_headers: HeaderMap,
        accept_invalid_certs: bool,
        timeout_secs: Option<u64>,
        tls_root_mode: TlsRootMode,
    ) -> Result<Client> {
        let mut builder = Client::builder()
            .tls_backend_rustls()
            .redirect(Self::redirect_policy())
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(None);

        if matches!(tls_root_mode, TlsRootMode::StaticWebpkiRoots) {
            builder = builder.tls_backend_preconfigured(Self::build_static_webpki_tls_config()?);
        }

        // Настройка прокси
        if let Some(proxy_str) = proxy_url {
            let proxy = Proxy::all(proxy_str)?;
            builder = builder.proxy(proxy);
        } else {
            builder = builder.no_proxy();
        }

        builder = builder.default_headers(default_headers);

        // SSL/TLS
        if accept_invalid_certs {
            builder = builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }

        // Настройка таймаута
        if let Some(secs) = timeout_secs {
            builder = builder
                .timeout(Duration::from_secs(secs))
                .connect_timeout(Duration::from_secs(secs.min(30)));
        }

        Ok(builder.build()?)
    }

    /// Ходим по редиректам, но не даём увести защищённый адрес на открытый: токен
    /// подписки в таком переезде уехал бы по сети открытым текстом. Подписки, которые
    /// пользователь сам задал через `http://`, работают как работали — понижением
    /// считается только переход `https` -> `http`.
    fn redirect_policy() -> reqwest::redirect::Policy {
        reqwest::redirect::Policy::custom(|attempt| {
            if redirect_is_a_downgrade(attempt.previous().first(), attempt.url()) {
                return attempt.error(DOWNGRADE_REFUSED);
            }

            if attempt.previous().len() > MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }

            attempt.follow()
        })
    }

    fn build_static_webpki_tls_config() -> Result<rustls::ClientConfig> {
        let root_store = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config =
            rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()?
                .with_root_certificates(root_store)
                .with_no_client_auth();

        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(config)
    }

    /// `single_use` — запрос одноразовый (защищённый канал): сервер, уже
    /// принявший его, второй такой же отбросит как повтор.
    fn should_retry_with_static_webpki_roots(err: &anyhow::Error, single_use: bool) -> bool {
        // Одноразовый запрос повторяется, только если так и не ушёл: сертификат
        // проверяется при установке соединения.
        let never_sent = || {
            err.chain().any(|e| {
                e.downcast_ref::<reqwest::Error>()
                    .is_some_and(reqwest::Error::is_connect)
            })
        };
        if (single_use && !never_sent()) || err.chain().any(Self::is_legacy_tls_protocol_error) {
            return false;
        }

        // Наш отказ от понижения до http — не беда с сертификатами. Проверка ниже
        // ищет ключевые слова во всей цепочке, а в ней есть хост запроса: хост, где
        // случайно встретилось `ssl` или `crl`, запустил бы бессмысленный второй
        // запрос к панели.
        if err.chain().any(|e| e.to_string().contains(DOWNGRADE_REFUSED)) {
            return false;
        }

        err.chain().any(|e| {
            let msg = e.to_string().to_ascii_lowercase();
            [
                "certificate",
                "cert",
                "tls",
                "ssl",
                "rustls",
                "webpki",
                "revocation",
                "ocsp",
                "crl",
                "issuer",
                "unknownissuer",
            ]
            .iter()
            .any(|kw| msg.contains(kw))
        })
    }

    fn context_reqwest_error(mut err: reqwest::Error, context: &'static str) -> anyhow::Error {
        // reqwest печатает в тексте ошибки полный адрес запроса — с токеном
        // подписки. От адреса остаются схема и хост: для разбора их хватает, а
        // текст ошибки уходит в журнал и в окна.
        if let Some(url) = err.url_mut() {
            url.set_path("");
            url.set_query(None);
            url.set_fragment(None);
            let _ = url.set_username("");
            let _ = url.set_password(None);
        }
        let legacy_tls = Self::is_legacy_tls_protocol_error(&err);
        let err = anyhow::Error::new(err).context(context);

        if legacy_tls {
            err.context("Subscription server uses legacy TLS; only TLS 1.2/1.3 is supported. TLS 1.0/1.1 is insecure")
        } else {
            err
        }
    }

    fn is_legacy_tls_protocol_error(err: &(dyn std::error::Error + 'static)) -> bool {
        let detail = format!("{err:#?}").to_ascii_lowercase();
        detail.contains("protocolversion") || detail.contains("protocol version")
    }

    pub async fn create_request(
        &self,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
    ) -> Result<Client> {
        self.create_request_with_tls_mode(
            proxy_type,
            timeout_secs,
            user_agent,
            accept_invalid_certs,
            TlsRootMode::PlatformVerifier,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_with_tls_mode(
        &self,
        url: &str,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        tls_root_mode: TlsRootMode,
        // clod: caller supplied headers (subscription identity, see config::sub_headers)
        custom_headers: Option<&HeaderMap>,
        // clod: тело запроса — тогда это POST (отчёт клиента по защищённому каналу)
        body: Option<&str>,
    ) -> Result<HttpResponse> {
        let mut parsed = Url::parse(url)?;
        let mut extra_headers = HeaderMap::new();

        // clod:headers begin
        if let Some(custom) = custom_headers {
            for (key, value) in custom.iter() {
                extra_headers.insert(key.clone(), value.clone());
            }
        }
        // clod:headers end

        if !parsed.username().is_empty() {
            let username = percent_encoding::percent_decode_str(parsed.username())
                .decode_utf8_lossy()
                .into_owned();
            let password = percent_encoding::percent_decode_str(parsed.password().unwrap_or_default())
                .decode_utf8_lossy()
                .into_owned();
            let auth_str = format!("{}:{}", username, password);
            let encoded = general_purpose::STANDARD.encode(auth_str);
            extra_headers.insert("Authorization", HeaderValue::from_str(&format!("Basic {}", encoded))?);
        }

        parsed.set_username("").ok();
        parsed.set_password(None).ok();

        // Создание запроса
        let client = self
            .create_request_with_tls_mode(
                proxy_type,
                timeout_secs,
                user_agent,
                accept_invalid_certs,
                tls_root_mode,
            )
            .await?;

        let mut request_builder = match body {
            Some(body) => client
                .post(parsed)
                .header(reqwest::header::CONTENT_TYPE, "text/plain")
                .body(body.to_owned()),
            None => client.get(parsed),
        };

        for (key, value) in extra_headers.iter() {
            request_builder = request_builder.header(key, value);
        }

        let response = match request_builder.send().await {
            Ok(resp) => resp,
            Err(e) => {
                return Err(Self::context_reqwest_error(e, "Request failed"));
            }
        };

        let status = response.status();
        let headers = response.headers().to_owned();
        let body = Self::read_capped(response, MAX_BODY).await?.into();

        Ok(HttpResponse::new(status, headers, body))
    }

    /// Тело ответа, но не больше `cap` байт после распаковки: бесконечный или
    /// огромный ответ не должен съесть память. Текст декодируется ровно как
    /// `Response::text` — по charset из `content-type`, по умолчанию UTF-8.
    async fn read_capped(response: reqwest::Response, cap: usize) -> Result<std::string::String> {
        let kind = response.headers().get(reqwest::header::CONTENT_TYPE).cloned();
        let body = Self::read_capped_bytes(response, cap).await?;

        let mut rebuilt = tauri::http::Response::new(body);
        if let Some(kind) = kind {
            rebuilt.headers_mut().insert(reqwest::header::CONTENT_TYPE, kind);
        }
        reqwest::Response::from(rebuilt)
            .text()
            .await
            .map_err(|e| Self::context_reqwest_error(e, "Failed to read response body"))
    }

    /// Тело ответа байтами, но не больше `cap` байт после распаковки.
    pub(crate) async fn read_capped_bytes(mut response: reqwest::Response, cap: usize) -> Result<Vec<u8>> {
        let too_large = || anyhow::anyhow!("the response body is larger than {} MiB", cap >> 20);
        if response.content_length().is_some_and(|len| len > cap as u64) {
            return Err(too_large());
        }

        let read_failed = |e| Self::context_reqwest_error(e, "Failed to read response body");
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(read_failed)? {
            if body.len() + chunk.len() > cap {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    async fn create_request_with_tls_mode(
        &self,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        tls_root_mode: TlsRootMode,
    ) -> Result<Client> {
        let proxy_url: Option<std::string::String> = match proxy_type {
            ProxyType::None => None,
            ProxyType::Localhost => {
                let port = Config::effective_mixed_port().await;
                Some(format!("http://127.0.0.1:{port}"))
            }
            ProxyType::System => system_proxy_url(),
        };

        let mut headers = HeaderMap::new();

        // Установка User-Agent
        if let Some(ua) = user_agent {
            headers.insert(USER_AGENT, HeaderValue::from_str(ua.as_str())?);
        } else {
            // clod: fork identity, must stay in sync with panel SRR rules
            headers.insert(
                USER_AGENT,
                HeaderValue::from_str(crate::utils::hwid::user_agent().as_str())?,
            );
        }

        self.build_client(proxy_url, headers, accept_invalid_certs, timeout_secs, tls_root_mode)
    }

    /// clod: запрос с прерыванием, которому можно добавить свои заголовки
    /// (используется для заголовков опознания подписки).
    pub async fn get_with_interrupt_and_headers(
        &self,
        url: &str,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        custom_headers: Option<&HeaderMap>,
    ) -> Result<HttpResponse> {
        self.send_with_fallback(
            url,
            proxy_type,
            timeout_secs,
            user_agent,
            accept_invalid_certs,
            custom_headers,
            None,
            false,
        )
        .await
    }

    /// clod: то же для одноразового запроса защищённого канала: после того как
    /// запрос ушёл, он второй раз не отправляется.
    pub async fn get_once_with_headers(
        &self,
        url: &str,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        custom_headers: Option<&HeaderMap>,
    ) -> Result<HttpResponse> {
        self.send_with_fallback(
            url,
            proxy_type,
            timeout_secs,
            user_agent,
            accept_invalid_certs,
            custom_headers,
            None,
            true,
        )
        .await
    }

    /// clod: POST с телом — отчёт клиента по защищённому каналу, одноразовый.
    #[allow(clippy::too_many_arguments)]
    pub async fn post_with_interrupt_and_headers(
        &self,
        url: &str,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        custom_headers: Option<&HeaderMap>,
        body: &str,
    ) -> Result<HttpResponse> {
        self.send_with_fallback(
            url,
            proxy_type,
            timeout_secs,
            user_agent,
            accept_invalid_certs,
            custom_headers,
            Some(body),
            true,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_with_fallback(
        &self,
        url: &str,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        custom_headers: Option<&HeaderMap>,
        body: Option<&str>,
        single_use: bool,
    ) -> Result<HttpResponse> {
        let platform_result = self
            .send_with_tls_mode(
                url,
                proxy_type,
                timeout_secs,
                user_agent.clone(),
                accept_invalid_certs,
                TlsRootMode::PlatformVerifier,
                custom_headers,
                body,
            )
            .await;

        match platform_result {
            Ok(response) => Ok(response),
            Err(err) if !accept_invalid_certs && Self::should_retry_with_static_webpki_roots(&err, single_use) => self
                .send_with_tls_mode(
                    url,
                    proxy_type,
                    timeout_secs,
                    user_agent,
                    accept_invalid_certs,
                    TlsRootMode::StaticWebpkiRoots,
                    custom_headers,
                    body,
                )
                .await
                .map_err(|fallback_err| {
                    fallback_err.context("static webpki roots fallback failed after platform TLS verifier failed")
                }),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod redirect_tests {
    use super::redirect_is_a_downgrade;
    use tauri::Url;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("тестовый адрес разбирается")
    }

    #[test]
    fn https_must_not_be_led_to_http() {
        assert!(redirect_is_a_downgrade(
            Some(&url("https://panel.example/sub")),
            &url("http://panel.example/sub")
        ));
        assert!(redirect_is_a_downgrade(
            Some(&url("https://panel.example/sub")),
            &url("http://other.example/sub")
        ));
    }

    #[test]
    fn a_subscription_that_started_as_http_keeps_working() {
        assert!(!redirect_is_a_downgrade(
            Some(&url("http://panel.example/sub")),
            &url("http://panel.example/moved")
        ));
        assert!(!redirect_is_a_downgrade(
            Some(&url("http://panel.example/sub")),
            &url("https://panel.example/moved")
        ));
    }

    #[test]
    fn https_to_https_is_followed() {
        assert!(!redirect_is_a_downgrade(
            Some(&url("https://panel.example/sub")),
            &url("https://mirror.example/sub")
        ));
    }

    #[test]
    fn without_a_first_address_there_is_nothing_to_downgrade() {
        assert!(!redirect_is_a_downgrade(None, &url("http://panel.example/sub")));
    }
}

#[cfg(test)]
mod retry_tests {
    use super::NetworkManager;

    #[test]
    fn only_a_single_use_request_is_not_sent_again_after_it_left() {
        let cut = || {
            anyhow::anyhow!("peer closed connection without sending TLS close_notify")
                .context("Failed to read response body")
        };
        assert!(NetworkManager::should_retry_with_static_webpki_roots(&cut(), false));
        assert!(!NetworkManager::should_retry_with_static_webpki_roots(&cut(), true));
    }
}

/// Настоящие обмены по сети через стек приложения: под Windows такой тестовый
/// бинарник на раннере CI не запускается (`STATUS_ENTRYPOINT_NOT_FOUND`), как и
/// проверка заголовков подписки.
#[cfg(all(test, not(windows)))]
#[allow(clippy::expect_used)]
mod transport_tests {
    use super::{NetworkManager, ProxyType};

    /// Потолок тела ответа: 64 МиБ.
    const CAP: usize = 64 << 20;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
    };

    /// Принимает одно соединение, дочитывает запрос и отвечает `head` и `body`.
    async fn answer_once(head: &'static str, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("порт для теста");
        let port = listener.local_addr().expect("адрес слушателя").port();
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while let Ok(read) = stream.read(&mut buffer).await {
                request.extend_from_slice(&buffer[..read]);
                // Начало TLS (не текст HTTP) — отвечаем сразу, рукопожатия не будет.
                let tls = request.first().is_some_and(|b| !b.is_ascii_alphabetic());
                if read == 0 || tls || request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&body).await;
            let _ = stream.shutdown().await;
        });
        format!("127.0.0.1:{port}")
    }

    async fn get(url: &str) -> anyhow::Result<super::HttpResponse> {
        NetworkManager::new()
            .get_with_interrupt_and_headers(url, ProxyType::None, Some(10), Some("test".into()), false, None)
            .await
    }

    #[tokio::test]
    async fn an_error_keeps_the_host_but_not_the_subscription_token() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("порт для теста");
        let port = listener.local_addr().expect("адрес слушателя").port();
        drop(listener);

        let text = get(&format!(
            "http://user:pass@127.0.0.1:{port}/sub/a7Kd93mQz1Lp0Xr8?flag=q9Zx81"
        ))
        .await
        .err()
        .map(|e| format!("{e:?} {e:#}"))
        .unwrap_or_default();

        assert!(text.contains("127.0.0.1"), "{text}");
        for secret in ["a7Kd93mQz1Lp0Xr8", "q9Zx81", "pass"] {
            assert!(!text.contains(secret), "{secret}: {text}");
        }
    }

    #[tokio::test]
    async fn an_endless_body_is_cut_off() {
        let addr = answer_once("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n", vec![b'a'; CAP + 1]).await;
        let text = get(&format!("http://{addr}/sub"))
            .await
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(text.contains("larger than"), "{text}");
    }

    #[tokio::test]
    async fn the_body_is_decoded_by_its_charset() {
        // «Привет» в windows-1251
        let body = vec![0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2];
        let addr = answer_once(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=windows-1251\r\nContent-Length: 6\r\n\r\n",
            body,
        )
        .await;
        let response = get(&format!("http://{addr}/sub")).await.expect("ответ пришёл");
        assert_eq!(response.text_with_charset().expect("текст"), "Привет");
    }

    #[tokio::test]
    async fn a_failed_tls_handshake_is_a_failure_before_the_request_left() {
        let addr = answer_once("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n", Vec::new()).await;
        let err = get(&format!("https://{addr}/sub")).await.err();
        assert!(
            err.as_ref().is_some_and(|err| err.chain().any(|e| e
                .downcast_ref::<reqwest::Error>()
                .is_some_and(reqwest::Error::is_connect))),
            "{err:?}"
        );
    }
}
