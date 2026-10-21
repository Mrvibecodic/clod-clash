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

    pub fn text_with_charset(&self) -> &str {
        &self.body
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

pub(crate) const MAX_REDIRECTS: usize = 10;

/// Больше этого тело ответа не читается: подписка столько не весит.
const MAX_BODY: usize = 64 << 20;

/// Тело ответа больше потолка (в байтах).
#[derive(Debug)]
pub(crate) struct BodyTooLarge(usize);

impl std::fmt::Display for BodyTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&clash_verge_i18n::t!(
            "errors.responseTooLarge",
            mib = (self.0 >> 20).to_string()
        ))
    }
}

impl std::error::Error for BodyTooLarge {}

pub(crate) const DOWNGRADE_REFUSED: &str = "redirect from https to http is refused";

/// Сервер принял запрос и закрыл его, не ответив: соединение (и TLS) уже
/// было, запрос ушёл, ответа нет — конец потока или сброс (HTTP/1.1), сброс
/// потока самим сервером (HTTP/2). Ошибка соединения, рукопожатия TLS и
/// таймаут — не это: там сервер мог запроса и не услышать. Обрыв на чтении
/// тела — тоже нет: сервер уже ответил.
pub(crate) fn hung_up(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|e| e.downcast_ref::<reqwest::Error>())
        .any(|e| e.is_request() && !e.is_connect() && !e.is_timeout() && closed_unanswered(e))
}

/// Причина под ошибкой запроса — закрытие без ответа. Ошибки hyper и h2 —
/// не наших зависимостей, их не опознать по типу, только по тексту.
fn closed_unanswered(err: &(dyn std::error::Error + 'static)) -> bool {
    std::iter::successors(err.source(), |e| e.source()).any(|e| {
        e.downcast_ref::<std::io::Error>().map_or_else(
            || {
                let text = e.to_string();
                text == "connection closed before message completed" || text.starts_with("stream error received: ")
            },
            |io| {
                matches!(
                    io.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                )
            },
        )
    })
}

/// Уводит ли редирект с закрытого адреса на открытый.
///
/// Смотрим на самый первый адрес цепочки, а не на предыдущий шаг: важно, что обещал
/// пользователю его собственный адрес подписки. Если он изначально был `http://`,
/// понижать нечего и цепочка идёт как раньше.
pub(crate) fn redirect_is_a_downgrade(first: Option<&Url>, next: &Url) -> bool {
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

/// Ошибка TLS под ошибкой запроса. rustls отдаёт её внутри `io::Error`, а тот
/// своё содержимое причиной не называет.
pub(crate) fn tls_failure<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a rustls::Error> {
    if let Some(tls) = err.downcast_ref::<rustls::Error>() {
        return Some(tls);
    }
    tls_failure(err.downcast_ref::<std::io::Error>()?.get_ref()?)
}

pub struct NetworkManager;

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Сервер подписки говорит только устаревшим TLS 1.0/1.1. Вид ошибки — тип, а
/// не слова в её тексте: текст переведён и на другом языке другой.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyTls;

impl std::fmt::Display for LegacyTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&clash_verge_i18n::t!("errors.legacyTls"))
    }
}

impl std::error::Error for LegacyTls {}

impl NetworkManager {
    pub const fn new() -> Self {
        Self
    }

    fn client_builder(
        &self,
        proxy_url: Option<std::string::String>,
        default_headers: HeaderMap,
        accept_invalid_certs: bool,
        timeout_secs: Option<u64>,
        tls_root_mode: TlsRootMode,
    ) -> Result<reqwest::ClientBuilder> {
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

        Ok(builder)
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
        if single_use && !never_sent() {
            return false;
        }

        // Вшитые корни помогают, только когда корня нет в системе или её проверка
        // сама не сработала. Отзыв, срок и чужое имя они не исправят, а отзыв и
        // не проверяют вовсе.
        err.chain().filter_map(tls_failure).any(|tls| {
            matches!(
                tls,
                rustls::Error::InvalidCertificate(
                    rustls::CertificateError::UnknownIssuer | rustls::CertificateError::Other(_)
                ) | rustls::Error::General(_)
            )
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

        if legacy_tls { err.context(LegacyTls) } else { err }
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
    ) -> Result<Client> {
        self.create_request_with_tls_mode(
            proxy_type,
            timeout_secs,
            user_agent,
            false,
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
        let too_large = || anyhow::Error::new(BodyTooLarge(cap));
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
        let (builder, _) = self
            .request_builder(
                proxy_type,
                timeout_secs,
                user_agent,
                accept_invalid_certs,
                tls_root_mode,
            )
            .await?;
        Ok(builder.build()?)
    }

    /// Клиент, который ходит только туда, куда пускает `redirect` на каждом
    /// шаге, а при соединении мимо прокси — и `resolver`.
    pub async fn create_restricted_request(
        &self,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        redirect: reqwest::redirect::Policy,
        resolver: impl reqwest::dns::Resolve + 'static,
    ) -> Result<Client> {
        let (builder, direct) = self
            .request_builder(proxy_type, timeout_secs, None, false, TlsRootMode::PlatformVerifier)
            .await?;
        let builder = builder.redirect(redirect);
        let builder = if direct {
            builder.dns_resolver(resolver)
        } else {
            builder
        };
        Ok(builder.build()?)
    }

    /// Сборка клиента и то, идёт ли он мимо прокси.
    async fn request_builder(
        &self,
        proxy_type: ProxyType,
        timeout_secs: Option<u64>,
        user_agent: Option<String>,
        accept_invalid_certs: bool,
        tls_root_mode: TlsRootMode,
    ) -> Result<(reqwest::ClientBuilder, bool)> {
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

        let direct = proxy_url.is_none();
        let builder = self.client_builder(proxy_url, headers, accept_invalid_certs, timeout_secs, tls_root_mode)?;
        Ok((builder, direct))
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

    /// Ошибка проверки сертификата так, как её приносит запрос: rustls внутри
    /// `io::Error` рукопожатия, а тот — внутри `io::Error` соединения.
    fn refused_certificate(cause: rustls::CertificateError) -> anyhow::Error {
        refused_handshake(rustls::Error::InvalidCertificate(cause))
    }

    fn refused_handshake(cause: rustls::Error) -> anyhow::Error {
        anyhow::Error::new(std::io::Error::other(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            cause,
        )))
        .context("Request failed")
    }

    #[test]
    fn only_a_single_use_request_is_not_sent_again_after_it_left() {
        let unknown = || refused_certificate(rustls::CertificateError::UnknownIssuer);
        assert!(NetworkManager::should_retry_with_static_webpki_roots(&unknown(), false));
        assert!(!NetworkManager::should_retry_with_static_webpki_roots(&unknown(), true));
    }

    #[test]
    fn only_a_missing_root_or_a_failed_system_check_is_asked_again_with_the_bundled_roots() {
        use rustls::CertificateError;

        let other = rustls::OtherError(std::sync::Arc::new(std::io::Error::other("CERT_E_CHAINING")));
        for (case, err, retry) in [
            (
                "корня нет в системе",
                refused_certificate(CertificateError::UnknownIssuer),
                true,
            ),
            (
                "прочий отказ проверки системы",
                refused_certificate(CertificateError::Other(other)),
                true,
            ),
            (
                "проверка системы недоступна",
                refused_handshake(rustls::Error::General("verification was unavailable".into())),
                true,
            ),
            ("отозван", refused_certificate(CertificateError::Revoked), false),
            ("просрочен", refused_certificate(CertificateError::Expired), false),
            (
                "не для этого имени",
                refused_certificate(CertificateError::NotValidForName),
                false,
            ),
            (
                "не для сервера",
                refused_certificate(CertificateError::InvalidPurpose),
                false,
            ),
            (
                "таймаут хоста со словами tls и ssl в имени",
                anyhow::anyhow!("operation timed out")
                    .context("error sending request for url (https://ssl.tls-cert.example.com/)"),
                false,
            ),
            (
                "оборванное тело",
                anyhow::anyhow!("peer closed connection without sending TLS close_notify")
                    .context("Failed to read response body"),
                false,
            ),
            (
                "отказ от понижения до http",
                anyhow::anyhow!(super::DOWNGRADE_REFUSED).context("Request failed"),
                false,
            ),
        ] {
            assert_eq!(
                NetworkManager::should_retry_with_static_webpki_roots(&err, false),
                retry,
                "{case}"
            );
        }
    }
}

/// Настоящие обмены по сети через стек приложения: под Windows такой тестовый
/// бинарник на раннере CI не запускается (`STATUS_ENTRYPOINT_NOT_FOUND`), как и
/// проверка заголовков подписки.
#[cfg(all(test, not(windows)))]
#[allow(clippy::expect_used)]
pub(crate) mod transport_tests {
    use super::{NetworkManager, ProxyType, hung_up};

    /// Потолок тела ответа: 64 МиБ.
    const CAP: usize = 64 << 20;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
    };

    /// Принимает одно соединение, дочитывает запрос и передаёт соединение `then`.
    async fn accept_once<F, Fut>(then: F) -> String
    where
        F: FnOnce(TcpStream) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
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
            then(stream).await;
        });
        format!("127.0.0.1:{port}")
    }

    /// Принимает одно соединение, дочитывает запрос и отвечает `head` и `body`.
    async fn answer_once(head: &'static str, body: Vec<u8>) -> String {
        accept_once(move |mut stream| async move {
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&body).await;
            let _ = stream.shutdown().await;
        })
        .await
    }

    /// Самоподписанный сертификат на 127.0.0.1 и localhost и его ключ (DER, base64).
    const SELF_SIGNED_CERT: &str = "MIIB0TCCAXigAwIBAgIUUK2t9TGftqAqW5oBiO3MUAmQfiAwCgYIKoZIzj0EAwIwJjEkMCIGA1UEAwwbQ2xvZCBDbGFzaCBzZWxmLXNpZ25lZCB0ZXN0MCAXDTI2MTAyMDIyMjAwNloYDzIxMjYwOTI2MjIyMDA2WjAmMSQwIgYDVQQDDBtDbG9kIENsYXNoIHNlbGYtc2lnbmVkIHRlc3QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAARxqUG9F+JWSj0mrQeKJRxVcPQW0rbeL+/4fQ78pZWGXKHIu/Pwf1v4eTvb2qU9x7YvjGdH74edlHFxvKCNo7X8o4GBMH8wHQYDVR0OBBYEFDKZsLtlthEDrzIel7GfHXqW7f7xMB8GA1UdIwQYMBaAFDKZsLtlthEDrzIel7GfHXqW7f7xMBoGA1UdEQQTMBGCCWxvY2FsaG9zdIcEfwAAATAMBgNVHRMBAf8EAjAAMBMGA1UdJQQMMAoGCCsGAQUFBwMBMAoGCCqGSM49BAMCA0cAMEQCIBHFb2m2MPq9hOIFfi4/KItIko/5qhCZIKsBRuLMCIW0AiAZ4w0A9j9y62KQzJcpjIXdC4d7dJ4BKxpxFjmjbdaRDg==";
    const SELF_SIGNED_KEY: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgnQdKzqUPPJYcnwB1v4+tJz6E5Rp4TGAq8VogbHg1SfqhRANCAARxqUG9F+JWSj0mrQeKJRxVcPQW0rbeL+/4fQ78pZWGXKHIu/Pwf1v4eTvb2qU9x7YvjGdH74edlHFxvKCNo7X8";

    /// Сервер TLS с самоподписанным сертификатом: принимает одно соединение,
    /// дочитывает запрос и отвечает `head`. Адрес — `127.0.0.1:порт`.
    pub(crate) fn self_signed_once(head: impl Into<String>) -> String {
        use base64::Engine as _;
        use std::io::{Read as _, Write as _};

        let decode = |b64: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .expect("тестовые данные")
        };
        let config =
            rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("версии TLS")
                .with_no_client_auth()
                .with_single_cert(
                    vec![rustls::pki_types::CertificateDer::from(decode(SELF_SIGNED_CERT))],
                    rustls::pki_types::PrivatePkcs8KeyDer::from(decode(SELF_SIGNED_KEY)).into(),
                )
                .expect("сертификат сервера");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("порт для теста");
        let addr = listener.local_addr().expect("адрес слушателя").to_string();
        let head = head.into();
        std::thread::spawn(move || {
            let Ok((socket, _)) = listener.accept() else {
                return;
            };
            let Ok(connection) = rustls::ServerConnection::new(std::sync::Arc::new(config)) else {
                return;
            };
            let mut tls = rustls::StreamOwned::new(connection, socket);
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match tls.read(&mut buffer) {
                    Ok(0) | Err(_) => return,
                    Ok(read) => request.extend_from_slice(&buffer[..read]),
                }
            }
            let _ = tls.write_all(head.as_bytes());
            tls.conn.send_close_notify();
            let _ = tls.flush();
        });
        addr
    }

    #[tokio::test]
    async fn a_certificate_the_system_does_not_know_is_asked_again_with_the_bundled_roots() {
        let addr = self_signed_once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        let client = NetworkManager::new()
            .create_request(ProxyType::None, Some(10), Some("test".into()))
            .await
            .expect("клиент");
        let err = client
            .get(format!("https://{addr}/sub"))
            .send()
            .await
            .map_err(anyhow::Error::new)
            .expect_err("самоподписанный сертификат не принят");
        assert!(
            matches!(
                err.chain().find_map(super::tls_failure),
                Some(rustls::Error::InvalidCertificate(
                    rustls::CertificateError::UnknownIssuer | rustls::CertificateError::Other(_)
                ))
            ),
            "{err:?}"
        );
        assert!(NetworkManager::should_retry_with_static_webpki_roots(&err, false));
    }

    /// Ошибка одноразового запроса, как у защищённого канала, с таймаутом в секунду.
    async fn once(addr: &str) -> anyhow::Error {
        NetworkManager::new()
            .get_once_with_headers(
                &format!("http://{addr}/c1/k/s/e"),
                ProxyType::None,
                Some(1),
                Some("test".into()),
                false,
                None,
            )
            .await
            .err()
            .unwrap_or_else(|| anyhow::anyhow!("сервер ответил"))
    }

    /// Защищённый запрос, который сервер принял и закрыл, ничего не ответив.
    pub(crate) async fn a_hung_up_request() -> anyhow::Error {
        once(&answer_once("", Vec::new()).await).await
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
        assert_eq!(response.text_with_charset(), "Привет");
    }

    #[tokio::test]
    async fn a_failed_tls_handshake_is_a_failure_before_the_request_left() {
        let addr = answer_once("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n", Vec::new()).await;
        let err = get(&format!("https://{addr}/sub")).await.err();
        assert!(
            err.as_ref().is_some_and(|err| err.chain().any(|e| e
                .downcast_ref::<reqwest::Error>()
                .is_some_and(reqwest::Error::is_connect))
                && !hung_up(err)),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn only_a_request_closed_unanswered_is_a_hang_up() {
        let reset = accept_once(|stream| async move {
            let _ = stream.set_zero_linger();
        })
        .await;
        let held = accept_once(|stream| async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            drop(stream);
        })
        .await;
        let cut = answer_once("HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n", b"abc".to_vec()).await;
        let refused = TcpListener::bind("127.0.0.1:0")
            .await
            .and_then(|listener| listener.local_addr())
            .map(|addr| addr.to_string())
            .unwrap_or_default();

        assert!(hung_up(&a_hung_up_request().await));
        for (case, addr, want) in [
            ("сброс", reset, true),
            ("таймаут", held, false),
            ("оборванное тело", cut, false),
            ("отказ в соединении", refused, false),
        ] {
            let err = once(&addr).await;
            assert_eq!(hung_up(&err), want, "{case}: {err:#}");
        }
    }

    /// Ошибка с причиной — как ошибка hyper над ошибкой h2.
    #[derive(Debug)]
    struct Layer(&'static str, Option<Box<Self>>);

    impl std::fmt::Display for Layer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Layer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_deref().map(|e| e as _)
        }
    }

    /// HTTP/2: поток сбросил сервер — обрыв; сбросил сам клиент — нет.
    #[test]
    fn a_stream_reset_by_the_server_is_closed_unanswered() {
        let request = |cause| {
            Layer(
                "client error (SendRequest)",
                Some(Box::new(Layer("http2 error", Some(Box::new(Layer(cause, None)))))),
            )
        };
        assert!(super::closed_unanswered(&request(
            "stream error received: unexpected internal error encountered"
        )));
        assert!(!super::closed_unanswered(&request(
            "stream error sent by user: unexpected internal error encountered"
        )));
    }
}
