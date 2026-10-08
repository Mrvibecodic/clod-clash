//! Протокол, транспорт и защита узла: «VLESS», «RAW (TCP)», «Reality» —
//! отдельными плашками и одной строкой «VLESS RAW (TCP) · Reality». Транспорта
//! и шифрования ядро не отдаёт (стоковое — тем более), поэтому всё берётся из
//! принятой сборки: её `proxies` и провайдеров узлов — и читается так, как его
//! читает clod-core. Где ядро может понять запись иначе, подписи нет: лучше
//! никакой, чем чужая.
//!
//! Правила сверены с кодом clod-core и стокового mihomo v1.19.32.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde::Serialize;
use serde_yaml_ng::{Mapping, Value};

use clash_verge_draft::SharedDraft;

use super::runtime::IRuntime;

/// Протокол, транспорт и защита узла; `text` — они же одной строкой, собранной
/// только здесь, чтобы плашки и строка не расходились.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProxyLabel {
    pub proto: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<&'static str>,
    pub text: String,
}

/// Подписи узлов сборки: самой подписки — по имени, провайдеров — по имени
/// провайдера и имени узла в нём, каким его показывает ядро.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Labels {
    pub proxies: HashMap<String, ProxyLabel>,
    pub providers: HashMap<String, HashMap<String, ProxyLabel>>,
}

/// Поле записи, как его находит декодер ядра: точный ключ, иначе без учёта
/// регистра (у узлов ещё и с `_` вместо `-`).
fn field<'a>(map: &'a Mapping, key: &str, underscores: bool) -> Option<&'a Value> {
    map.get(key).or_else(|| {
        map.iter()
            .find(|(k, _)| {
                k.as_str().is_some_and(|k| {
                    let k = if underscores { k.replace('_', "-") } else { k.to_owned() };
                    k.eq_ignore_ascii_case(key)
                })
            })
            .map(|(_, v)| v)
    })
}

fn opt<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    field(map, key, true)
}

/// Флаг, как его понимает ядро: `true` или ненулевое число.
fn on(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => {
            number.as_i64().is_some_and(|n| n != 0) || number.as_u64().is_some_and(|n| n != 0)
        }
        _ => false,
    }
}

/// Поле-строка, как его заполняет декодер ядра: строка как есть, число —
/// своей записью (значит, непустое).
const fn text_set(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(_)) => true,
        _ => false,
    }
}

/// Поле-число, как его заполняет декодер ядра: целое как есть, дробное —
/// отброшенной дробной частью, строка — `strconv.ParseInt(s, 0, 64)`.
fn int_set(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .map(|n| n != 0)
            .or_else(|| number.as_u64().map(|n| n != 0))
            .or_else(|| number.as_f64().map(|n| n.trunc() != 0.0))
            .unwrap_or(false),
        Some(Value::String(text)) => go_int_is_nonzero(text).unwrap_or(false),
        _ => false,
    }
}

/// Ненулевое ли целое по правилам Go для `ParseInt(s, 0, 64)`: знак,
/// приставки `0x`/`0o`/`0b`/`0`, подчёркивания между цифрами. `None` — ядро
/// такое не разберёт (и запись не примет).
fn go_int_is_nonzero(text: &str) -> Option<bool> {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let lower = body.to_ascii_lowercase();
    let (radix, digits, prefixed) = if let Some(rest) = lower.strip_prefix("0x") {
        (16, rest, true)
    } else if let Some(rest) = lower.strip_prefix("0o") {
        (8, rest, true)
    } else if let Some(rest) = lower.strip_prefix("0b") {
        (2, rest, true)
    } else if lower.len() > 1 && lower.starts_with('0') {
        (8, &lower[1..], true)
    } else {
        (10, lower.as_str(), false)
    };
    // Подчёркивание — только после цифры или приставки и не последним.
    let mut after_digit = prefixed;
    let mut any = false;
    for c in digits.chars() {
        if c == '_' {
            if !after_digit {
                return None;
            }
            after_digit = false;
        } else if c.is_digit(radix) {
            after_digit = true;
            any = true;
        } else {
            return None;
        }
    }
    if !any || digits.ends_with('_') {
        return None;
    }
    Some(digits.chars().any(|c| c != '0' && c != '_'))
}

/// Включён ли блок опций: хоть одно поле-строка или поле-число заполнено.
fn block_set(block: Option<&Value>, texts: &[&str], ints: &[&str]) -> bool {
    block.and_then(Value::as_mapping).is_some_and(|block| {
        texts.iter().any(|key| text_set(field(block, key, true)))
            || ints.iter().any(|key| int_set(field(block, key, true)))
    })
}

/// Имя протокола; типы, которых clod-core не знает, — без подписи.
fn proto_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "vless" => "VLESS",
        "vmess" => "VMess",
        "trojan" => "Trojan",
        "ss" => "Shadowsocks",
        "hysteria2" => "Hysteria2",
        "hysteria" => "Hysteria",
        "tuic" => "TUIC",
        "wireguard" => "WireGuard",
        "socks5" => "SOCKS",
        "http" => "HTTP",
        "anytls" => "AnyTLS",
        "ssh" => "SSH",
        "mieru" => "Mieru",
        _ => return None,
    })
}

/// Транспорт — только тот, что ядро у этого протокола делает (`switch
/// option.Network` в clod-core, с учётом регистра); прочее ядро ведёт по TCP.
/// `http` у ядра — HTTP/1.1-заголовки поверх TCP, а не HTTP/2.
fn transport(kind: &str, proxy: &Mapping) -> Option<&'static str> {
    let network = opt(proxy, "network").and_then(Value::as_str).unwrap_or("");
    let upgrade = |opts: Option<&Value>, underscores| {
        on(opts
            .and_then(Value::as_mapping)
            .and_then(|opts| field(opts, "v2ray-http-upgrade", underscores)))
    };
    Some(match (kind, network) {
        ("vless" | "vmess" | "trojan", "ws") if upgrade(opt(proxy, "ws-opts"), true) => "HTTPUpgrade",
        ("vless" | "vmess" | "trojan", "ws") => "WebSocket",
        ("vless" | "vmess" | "trojan", "grpc") => "gRPC",
        ("vless" | "vmess", "h2") => "HTTP/2",
        ("vless", "xhttp") => "XHTTP",
        ("vmess", "mkcp" | "kcp") => "mKCP",
        ("vmess", "mekya") => "Mekya",
        ("vless" | "vmess" | "trojan", _) => "RAW (TCP)",
        ("hysteria" | "hysteria2" | "tuic", _) => "QUIC",
        ("ss", _) => {
            let opts = opt(proxy, "plugin-opts");
            let mode = opts
                .and_then(Value::as_mapping)
                .and_then(|opts| field(opts, "mode", false))
                .and_then(Value::as_str);
            match (opt(proxy, "plugin").and_then(Value::as_str), mode) {
                (Some("v2ray-plugin"), Some("websocket")) if upgrade(opts, false) => "HTTPUpgrade",
                (Some("v2ray-plugin" | "gost-plugin"), Some("websocket")) => "WebSocket",
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Защита: Reality, TLSMirror, ShadowTLS, Restls, JLS — только когда ядро их
/// включает, иначе TLS, если он есть. Reality ядро не делает поверх WebSocket.
fn security(kind: &str, proxy: &Mapping) -> Option<&'static str> {
    match kind {
        "hysteria" | "hysteria2" | "tuic" => Some("TLS"),
        "socks5" | "http" => on(opt(proxy, "tls")).then_some("TLS"),
        "ss" => {
            let opts = opt(proxy, "plugin-opts");
            match opt(proxy, "plugin").and_then(Value::as_str) {
                Some("v2ray-plugin" | "gost-plugin")
                    if on(opts
                        .and_then(Value::as_mapping)
                        .and_then(|opts| field(opts, "tls", false))) =>
                {
                    Some("TLS")
                }
                Some("shadow-tls") => Some("ShadowTLS"),
                Some("restls") => Some("Restls"),
                Some("jls") => Some("JLS"),
                _ => None,
            }
        }
        "vless" | "vmess" | "trojan" | "anytls" => {
            if !(matches!(kind, "trojan" | "anytls") || on(opt(proxy, "tls"))) {
                return None;
            }
            let network = opt(proxy, "network").and_then(Value::as_str);
            // У XHTTP своя защита может быть у загрузки — какая сработает, не угадать.
            if kind == "vless"
                && network == Some("xhttp")
                && opt(proxy, "xhttp-opts")
                    .and_then(Value::as_mapping)
                    .and_then(|opts| field(opts, "download-settings", true))
                    .and_then(Value::as_mapping)
                    .is_some_and(|download| {
                        ["tls", "reality-opts", "shadow-tls-opts", "restls-opts", "jls-opts"]
                            .iter()
                            .any(|key| field(download, key, true).is_some())
                    })
            {
                return None;
            }
            let modes: Vec<&'static str> = [
                (
                    block_set(opt(proxy, "shadow-tls-opts"), &["password"], &["version"]),
                    "ShadowTLS",
                ),
                (
                    block_set(
                        opt(proxy, "restls-opts"),
                        &["password", "version-hint", "restls-script"],
                        &[],
                    ),
                    "Restls",
                ),
                (block_set(opt(proxy, "jls-opts"), &["username", "password"], &[]), "JLS"),
                (
                    kind != "anytls" && block_set(opt(proxy, "reality-opts"), &["public-key"], &[]),
                    "Reality",
                ),
                (
                    kind == "vmess" && block_set(opt(proxy, "tlsmirror-opts"), &["primary-key"], &[]),
                    "TLSMirror",
                ),
            ]
            .into_iter()
            .filter_map(|(active, mode)| active.then_some(mode))
            .collect();
            match modes.as_slice() {
                [] => Some("TLS"),
                ["Reality"] if network == Some("ws") => Some("TLS"),
                // Поверх mKCP ядро эти три не принимает.
                ["ShadowTLS" | "Restls" | "JLS"] if kind == "vmess" && matches!(network, Some("mkcp" | "kcp")) => None,
                [mode] => Some(mode),
                // Несколько сразу ядро не принимает.
                _ => None,
            }
        }
        _ => None,
    }
}

/// Протокол, транспорт и защита узла из его записи; без `type` или с типом,
/// которого нет в таблице, — `None` (остаётся тип от ядра).
pub fn label(proxy: &Mapping) -> Option<ProxyLabel> {
    // Тип ядро берёт по точному ключу и сравнивает с учётом регистра.
    let kind = proxy.get("type")?.as_str()?;
    let proto = proto_name(kind)?.to_owned();
    let transport = transport(kind, proxy);
    let security = security(kind, proxy);
    let mut text = proto.clone();
    if let Some(transport) = transport {
        text.push(' ');
        text.push_str(transport);
    }
    if let Some(security) = security {
        text.push_str(" · ");
        text.push_str(security);
    }
    Some(ProxyLabel {
        proto,
        transport,
        security,
        text,
    })
}

fn listed(proxies: Option<&Value>) -> impl Iterator<Item = &Mapping> {
    proxies
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(Value::as_mapping)
}

fn yaml_name(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.clone()),
        Value::Number(name) => Some(name.to_string()),
        _ => None,
    }
}

/// Что провайдер делает с узлами по дороге в ядро (`NewProxiesParser`).
struct Shaping<'a> {
    exclude_types: Vec<&'a str>,
    prefix: &'a str,
    suffix: &'a str,
}

impl<'a> Shaping<'a> {
    /// `None` — имена узлов не повторить: `proxy-name` и `override-expr`
    /// переписывают их (а `override-expr` — и что угодно ещё) по-своему.
    fn of(provider: &'a Mapping) -> Option<Self> {
        let text = |map: &'a Mapping, key: &str| match field(map, key, false) {
            None => Some(""),
            Some(value) => value.as_str(),
        };
        let exclude_types = text(provider, "exclude-type")?;
        let (mut prefix, mut suffix) = ("", "");
        if let Some(over) = field(provider, "override", false) {
            let over = over.as_mapping()?;
            let used = |key| {
                field(over, key, false).is_some_and(|value| value.as_sequence().is_none_or(|list| !list.is_empty()))
            };
            if used("proxy-name") || used("override-expr") {
                return None;
            }
            prefix = text(over, "additional-prefix")?;
            suffix = text(over, "additional-suffix")?;
        }
        Some(Self {
            exclude_types: exclude_types.split('|').filter(|t| !t.is_empty()).collect(),
            prefix,
            suffix,
        })
    }

    /// Узлы, какими их заводит ядро: без исключённых типов, первый из
    /// одноимённых, имя с приставками. Фильтры по имени не нужны: одноимённые
    /// они пропускают или отбрасывают разом, а отброшенных ядро не покажет.
    fn apply(&self, proxies: Option<&Value>) -> HashMap<String, ProxyLabel> {
        let mut seen = HashSet::new();
        let mut out = HashMap::new();
        for proxy in listed(proxies) {
            if !self.exclude_types.is_empty() {
                let Some(kind) = proxy.get("type").and_then(Value::as_str) else {
                    continue;
                };
                if self.exclude_types.iter().any(|t| t.eq_ignore_ascii_case(kind)) {
                    continue;
                }
            }
            let Some(name) = proxy.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !seen.insert(name) {
                continue;
            }
            if let Some(label) = label(proxy) {
                out.insert(format!("{}{name}{}", self.prefix, self.suffix), label);
            }
        }
        out
    }
}

/// Момент записи и длина файла: по ним видно, что файл переписан.
type FileState = Option<(SystemTime, u64)>;

fn file_state(path: &Path) -> FileState {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Ключ слияния `<<` ядро раскрывает, а мы — нет: с ним подписи не угадать.
fn has_merge(value: Option<&Value>) -> bool {
    value.is_some_and(crate::utils::help::contains_merge_key)
}

/// Файл провайдера, как его находит ядро: `path` как есть (относительный — от
/// папки ядра); у скачанного с пустым `path` — `proxies/<md5 адреса>`.
fn core_file(remote: bool, provider: &Mapping) -> Option<PathBuf> {
    use md5::Digest as _;
    let text = |key| match field(provider, key, false) {
        None => Some(String::new()),
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Number(number)) if !number.is_f64() => Some(number.to_string()),
        _ => None,
    };
    let path = text("path")?;
    let home = || crate::utils::dirs::app_home_dir().ok();
    if remote && path.is_empty() {
        let url = text("url")?;
        return Some(
            home()?
                .join("proxies")
                .join(hex::encode(md5::Md5::digest(url.as_bytes()))),
        );
    }
    let path = Path::new(&path);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        home()?.join(path)
    })
}

/// Подписи сборки, файлы провайдеров, из которых они прочитаны (с состоянием
/// до чтения), и не переписывался ли какой-то файл, пока его читали.
/// `own_files` — файлы скачанных провайдеров лежат у приложения: ядро запущено
/// им самим, а не службой (у службы они в её папке).
fn build(config: &Mapping, own_files: bool) -> (Labels, Vec<(PathBuf, FileState)>, bool) {
    let mut labels = Labels::default();
    let mut files = Vec::new();
    let mut steady = true;
    for proxy in listed(config.get("proxies")) {
        if proxy.contains_key("<<") || proxy.values().any(crate::utils::help::contains_merge_key) {
            continue;
        }
        if let (Some(name), Some(label)) = (proxy.get("name").and_then(Value::as_str), label(proxy)) {
            labels.proxies.entry(name.to_owned()).or_insert(label);
        }
    }
    for (name, provider) in config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(provider)) = (yaml_name(name), provider.as_mapping()) else {
            continue;
        };
        let Some(shaping) = Shaping::of(provider) else {
            continue;
        };
        let payload = field(provider, "payload", false);
        if has_merge(payload) {
            continue;
        }
        let kind = field(provider, "type", false).and_then(Value::as_str);
        let nodes = match kind {
            Some("inline") => shaping.apply(payload),
            Some("file" | "http") => {
                if kind == Some("http") && !own_files {
                    continue;
                }
                let Some(path) = core_file(kind == Some("http"), provider) else {
                    continue;
                };
                let before = file_state(&path);
                let doc = before
                    .and_then(|_| std::fs::read_to_string(&path).ok())
                    .and_then(|text| serde_yaml_ng::from_str::<Value>(&text).ok());
                steady &= file_state(&path) == before;
                files.push((path, before));
                let proxies = doc.as_ref().and_then(|doc| doc.get("proxies"));
                if has_merge(proxies) {
                    continue;
                }
                // `payload` у них — запасной набор, пока файл не прочитан ядром.
                // Какой из двух сейчас у ядра, не узнать: где они расходятся, подписи нет.
                let fallback = shaping.apply(payload);
                let mut nodes = shaping.apply(proxies);
                nodes.retain(|name, label| fallback.get(name).is_none_or(|other| other == label));
                nodes
            }
            _ => continue,
        };
        labels.providers.insert(name, nodes);
    }
    (labels, files, steady)
}

/// Последние подписи: по какой сборке, при каком запуске ядра и каких файлах
/// провайдеров собраны. Сборку держим, а не помним адрес: адрес освобождённой
/// мог бы достаться новой.
struct Cached {
    runtime: SharedDraft<IRuntime>,
    own_files: bool,
    files: Vec<(PathBuf, FileState)>,
    stamp: String,
    labels: Arc<Labels>,
}

static CACHE: Mutex<Option<Arc<Cached>>> = Mutex::new(None);

/// Подписи принятой сборки и их отпечаток. Пересобираются, только когда
/// сменилась сборка, способ запуска ядра или файл провайдера; файлы читаются
/// вне рантайма.
pub async fn current() -> (String, Arc<Labels>) {
    use crate::core::{CoreManager, manager::RunningMode};
    let runtime = super::Config::runtime().await.data_arc();
    let own_files = matches!(*CoreManager::global().get_running_mode(), RunningMode::Sidecar);
    let cached = CACHE.lock().clone();
    let fresh = tokio::task::spawn_blocking(move || {
        if let Some(cached) = cached
            && Arc::ptr_eq(&cached.runtime, &runtime)
            && cached.own_files == own_files
            && cached.files.iter().all(|(path, at)| file_state(path) == *at)
        {
            return (cached, true);
        }
        let (labels, files, steady) = runtime.config.as_ref().map_or_else(
            || (Labels::default(), Vec::new(), true),
            |config| build(config, own_files),
        );
        let stamp = {
            use std::hash::{Hash as _, Hasher as _};
            let mut hasher = std::hash::DefaultHasher::new();
            serde_json::to_string(&sorted(&labels))
                .unwrap_or_default()
                .hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        };
        let fresh = Arc::new(Cached {
            runtime,
            own_files,
            files,
            stamp,
            labels: Arc::new(labels),
        });
        (fresh, steady)
    })
    .await;
    match fresh {
        Ok((fresh, steady)) => {
            let out = (fresh.stamp.clone(), Arc::clone(&fresh.labels));
            // Файл переписывали, пока читали, — прочитанное не запоминаем.
            if steady {
                *CACHE.lock() = Some(fresh);
            }
            out
        }
        Err(_) => (String::new(), Arc::default()),
    }
}

/// Подписи в порядке имён: отпечаток не должен зависеть от порядка в таблице.
fn sorted(labels: &Labels) -> Vec<(&str, &str, &ProxyLabel)> {
    let mut out: Vec<_> = labels
        .proxies
        .iter()
        .map(|(name, label)| ("", name.as_str(), label))
        .chain(labels.providers.iter().flat_map(|(provider, nodes)| {
            nodes
                .iter()
                .map(move |(name, label)| (provider.as_str(), name.as_str(), label))
        }))
        .collect();
    out.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{build, go_int_is_nonzero, label};
    use serde_yaml_ng::Mapping;

    fn yaml(text: &str) -> Mapping {
        serde_yaml_ng::from_str(text).unwrap()
    }

    /// Плашки узла: протокол, транспорт, защита.
    fn parts(proxy: &str) -> (String, Option<&'static str>, Option<&'static str>) {
        let label = label(&yaml(proxy)).unwrap();
        (label.proto, label.transport, label.security)
    }

    const KEY: &str = "public-key: k";

    #[test]
    fn protocol_transport_and_security_are_read_as_the_core_reads_them() {
        for (proxy, proto, transport, security) in [
            ("{type: vless}", "VLESS", Some("RAW (TCP)"), None),
            (
                "{type: vless, network: tcp, tls: true, reality-opts: {KEY}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("Reality"),
            ),
            (
                "{type: vless, tls: 1, reality_opts: {public_key: k}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("Reality"),
            ),
            (
                "{type: vless, Network: grpc, TLS: true}",
                "VLESS",
                Some("gRPC"),
                Some("TLS"),
            ),
            ("{type: vless, tls: 0}", "VLESS", Some("RAW (TCP)"), None),
            ("{type: vless, reality-opts: {KEY}}", "VLESS", Some("RAW (TCP)"), None),
            (
                "{type: vless, tls: true, reality-opts: {public-key: ''}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("TLS"),
            ),
            (
                "{type: vless, network: ws, tls: true, reality-opts: {KEY}}",
                "VLESS",
                Some("WebSocket"),
                Some("TLS"),
            ),
            (
                "{type: vless, network: ws, ws-opts: {v2ray-http-upgrade: true}}",
                "VLESS",
                Some("HTTPUpgrade"),
                None,
            ),
            (
                "{type: vless, network: ws, ws_opts: {v2ray_http_upgrade: 1}}",
                "VLESS",
                Some("HTTPUpgrade"),
                None,
            ),
            ("{type: vless, network: WS}", "VLESS", Some("RAW (TCP)"), None),
            (
                "{type: vless, network: grpc, tls: true, reality-opts: {KEY}}",
                "VLESS",
                Some("gRPC"),
                Some("Reality"),
            ),
            ("{type: vless, network: xhttp}", "VLESS", Some("XHTTP"), None),
            ("{type: vless, network: splithttp}", "VLESS", Some("RAW (TCP)"), None),
            ("{type: vless, network: httpupgrade}", "VLESS", Some("RAW (TCP)"), None),
            ("{type: vless, network: http}", "VLESS", Some("RAW (TCP)"), None),
            (
                "{type: vless, network: h2, tls: true}",
                "VLESS",
                Some("HTTP/2"),
                Some("TLS"),
            ),
            ("{type: vless, network: kcp}", "VLESS", Some("RAW (TCP)"), None),
            (
                "{type: vless, tls: true, shadow-tls-opts: {password: p}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("ShadowTLS"),
            ),
            (
                "{type: vless, tls: true, jls-opts: {username: u}, reality-opts: {KEY}}",
                "VLESS",
                Some("RAW (TCP)"),
                None,
            ),
            (
                "{type: vmess, network: h2, tls: true}",
                "VMess",
                Some("HTTP/2"),
                Some("TLS"),
            ),
            ("{type: vmess, network: http}", "VMess", Some("RAW (TCP)"), None),
            ("{type: vmess, network: kcp}", "VMess", Some("mKCP"), None),
            (
                "{type: vmess, network: mkcp, tls: true, reality-opts: {KEY}}",
                "VMess",
                Some("mKCP"),
                Some("Reality"),
            ),
            ("{type: vmess, network: mekya}", "VMess", Some("Mekya"), None),
            ("{type: vmess, network: xhttp}", "VMess", Some("RAW (TCP)"), None),
            ("{type: vmess, network: quic}", "VMess", Some("RAW (TCP)"), None),
            ("{type: trojan}", "Trojan", Some("RAW (TCP)"), Some("TLS")),
            (
                "{type: trojan, network: grpc, reality-opts: {KEY}}",
                "Trojan",
                Some("gRPC"),
                Some("Reality"),
            ),
            (
                "{type: trojan, network: ws, reality-opts: {KEY}}",
                "Trojan",
                Some("WebSocket"),
                Some("TLS"),
            ),
            ("{type: trojan, network: h2}", "Trojan", Some("RAW (TCP)"), Some("TLS")),
            (
                "{type: trojan, restls-opts: {version-hint: tls13}}",
                "Trojan",
                Some("RAW (TCP)"),
                Some("Restls"),
            ),
            ("{type: ss}", "Shadowsocks", None, None),
            ("{type: ss, network: ws}", "Shadowsocks", None, None),
            (
                "{type: ss, plugin: obfs, plugin-opts: {mode: tls}}",
                "Shadowsocks",
                None,
                None,
            ),
            (
                "{type: ss, plugin: v2ray-plugin, plugin-opts: {mode: websocket}}",
                "Shadowsocks",
                Some("WebSocket"),
                None,
            ),
            (
                "{type: ss, plugin: v2ray-plugin, plugin-opts: {mode: websocket, v2ray-http-upgrade: true}}",
                "Shadowsocks",
                Some("HTTPUpgrade"),
                None,
            ),
            (
                "{type: ss, plugin: gost-plugin, plugin-opts: {mode: websocket, tls: true}}",
                "Shadowsocks",
                Some("WebSocket"),
                Some("TLS"),
            ),
            (
                "{type: ss, plugin: shadow-tls, plugin-opts: {password: p, version: 3}}",
                "Shadowsocks",
                None,
                Some("ShadowTLS"),
            ),
            (
                "{type: ss, plugin: restls, plugin-opts: {password: p}}",
                "Shadowsocks",
                None,
                Some("Restls"),
            ),
            ("{type: ss, plugin: kcptun}", "Shadowsocks", None, None),
            ("{type: hysteria2}", "Hysteria2", Some("QUIC"), Some("TLS")),
            ("{type: hysteria}", "Hysteria", Some("QUIC"), Some("TLS")),
            ("{type: tuic, network: ws}", "TUIC", Some("QUIC"), Some("TLS")),
            ("{type: wireguard}", "WireGuard", None, None),
            ("{type: socks5}", "SOCKS", None, None),
            ("{type: socks5, tls: true}", "SOCKS", None, Some("TLS")),
            ("{type: http, tls: true}", "HTTP", None, Some("TLS")),
            ("{type: anytls}", "AnyTLS", None, Some("TLS")),
            ("{type: anytls, reality-opts: {KEY}}", "AnyTLS", None, Some("TLS")),
            (
                "{type: anytls, shadow-tls-opts: {version: 3}}",
                "AnyTLS",
                None,
                Some("ShadowTLS"),
            ),
            ("{type: ssh}", "SSH", None, None),
            ("{type: mieru}", "Mieru", None, None),
            (
                "{type: vmess, tls: true, tlsmirror-opts: {primary-key: p}}",
                "VMess",
                Some("RAW (TCP)"),
                Some("TLSMirror"),
            ),
            (
                "{type: vmess, network: ws, tls: true, tlsmirror-opts: {primary-key: p}}",
                "VMess",
                Some("WebSocket"),
                Some("TLSMirror"),
            ),
            (
                "{type: vmess, network: mkcp, tls: true, tlsmirror_opts: {primary_key: 7}}",
                "VMess",
                Some("mKCP"),
                Some("TLSMirror"),
            ),
            (
                "{type: vmess, network: grpc, tls: true, tlsmirror-opts: {primary-key: ''}}",
                "VMess",
                Some("gRPC"),
                Some("TLS"),
            ),
            (
                "{type: vmess, tlsmirror-opts: {primary-key: p}}",
                "VMess",
                Some("RAW (TCP)"),
                None,
            ),
            (
                "{type: vmess, tls: true, reality-opts: {KEY}, tlsmirror-opts: {primary-key: p}}",
                "VMess",
                Some("RAW (TCP)"),
                None,
            ),
            (
                "{type: vless, tls: true, tlsmirror-opts: {primary-key: p}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("TLS"),
            ),
            (
                "{type: vmess, network: kcp, tls: true, shadow-tls-opts: {password: p}}",
                "VMess",
                Some("mKCP"),
                None,
            ),
            (
                "{type: vless, tls: true, reality-opts: {public-key: 123}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("Reality"),
            ),
            (
                "{type: vless, tls: true, shadow-tls-opts: {version: 3}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("ShadowTLS"),
            ),
            (
                "{type: vless, tls: true, shadow-tls-opts: {version: '0b1'}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("ShadowTLS"),
            ),
            (
                "{type: vless, tls: true, shadow-tls-opts: {version: '0x0'}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("TLS"),
            ),
            (
                "{type: vless, tls: true, shadow-tls-opts: {version: 0.5}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("TLS"),
            ),
            (
                "{type: vless, tls: true, jls-opts: {username: 0}}",
                "VLESS",
                Some("RAW (TCP)"),
                Some("JLS"),
            ),
            (
                "{type: vless, network: xhttp, tls: true, reality-opts: {KEY}}",
                "VLESS",
                Some("XHTTP"),
                Some("Reality"),
            ),
            (
                "{type: vless, network: xhttp, tls: true, xhttp-opts: {download-settings: {tls: false}}}",
                "VLESS",
                Some("XHTTP"),
                None,
            ),
            (
                "{type: vless, network: xhttp, tls: true, xhttp-opts: {download-settings: {path: /d}}}",
                "VLESS",
                Some("XHTTP"),
                Some("TLS"),
            ),
        ] {
            let proxy = proxy.replace("KEY", KEY);
            let (got_proto, got_transport, got_security) = parts(&proxy);
            assert_eq!(
                (got_proto.as_str(), got_transport, got_security),
                (proto, transport, security),
                "{proxy}"
            );
        }
        assert_eq!(label(&yaml("{name: a}")), None);
        for unknown in [
            "{type: ''}",
            "{type: snell}",
            "{type: direct}",
            "{type: shadowsocks}",
            "{type: VLESS}",
        ] {
            assert_eq!(label(&yaml(unknown)), None, "{unknown}");
        }
    }

    #[test]
    fn the_line_is_made_of_the_chips() {
        for (proxy, want) in [
            (
                "{type: vless, tls: true, reality-opts: {public-key: k}}",
                "VLESS RAW (TCP) · Reality",
            ),
            ("{type: vmess, network: ws}", "VMess WebSocket"),
            ("{type: hysteria2}", "Hysteria2 QUIC · TLS"),
            ("{type: anytls}", "AnyTLS · TLS"),
            ("{type: ss}", "Shadowsocks"),
        ] {
            assert_eq!(label(&yaml(proxy)).unwrap().text, want, "{proxy}");
        }
    }

    #[test]
    fn provider_nodes_are_labelled_as_the_core_names_them() {
        let dir = std::env::temp_dir().join(format!("clod-proxy-label-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("provider.yaml");
        std::fs::write(
            &file,
            "proxies:\n  - {name: A, type: ss}\n  - {name: F, type: vless, network: grpc, tls: true}\n",
        )
        .unwrap();
        let config = yaml(&format!(
            "proxies:\n  - {{name: A, type: trojan}}\n  - {{name: B}}\nproxy-providers:\n  \
             inline:\n    type: inline\n    exclude-type: ss|Vmess\n    override: {{additional-prefix: '[i] ', additional-suffix: ' *'}}\n    \
             payload:\n      - {{name: D, type: ss}}\n      - {{name: D, type: tuic}}\n      - {{name: D, type: vless}}\n      \
             - {{name: V, type: vmess}}\n      - {{name: 7, type: vless}}\n  \
             file:\n    type: file\n    path: {}\n    payload: [{{name: A, type: vless}}, {{name: F, type: vless, network: grpc, tls: true}}]\n  \
             web:\n    type: http\n    path: {}\n  \
             renamed:\n    type: inline\n    override: {{proxy-name: [{{pattern: a, target: b}}]}}\n    payload: [{{name: a, type: vless}}]\n  \
             gone:\n    type: file\n    path: {}\n",
            file.display(),
            file.display(),
            dir.join("missing.yaml").display()
        ));
        let (labels, files, steady) = build(&config, true);
        let (service, _, _) = build(&config, false);
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(labels.proxies.len(), 1, "{labels:?}");
        assert_eq!(labels.proxies["A"].text, "Trojan RAW (TCP) · TLS");
        let inline = &labels.providers["inline"];
        assert_eq!(inline.len(), 1, "{inline:?}");
        assert_eq!(inline["[i] D *"].text, "TUIC QUIC · TLS");
        // В запасном `payload` у A другой протокол — какой у ядра, не узнать.
        assert!(!labels.providers["file"].contains_key("A"));
        assert_eq!(labels.providers["file"]["F"].text, "VLESS gRPC · TLS");
        assert_eq!(labels.providers["web"]["A"].text, "Shadowsocks");
        assert_eq!(labels.providers["web"]["F"].text, "VLESS gRPC · TLS");
        assert!(!labels.providers.contains_key("renamed"));
        assert!(labels.providers["gone"].is_empty());
        assert_eq!(files.len(), 3);
        assert!(steady);
        assert!(!service.providers.contains_key("web"), "{service:?}");
        assert_eq!(service.providers["file"], labels.providers["file"]);
    }

    #[test]
    fn integers_are_read_like_go_parse_int_base_zero() {
        for (text, want) in [
            ("0", Some(false)),
            ("-0", Some(false)),
            ("3", Some(true)),
            ("+3", Some(true)),
            ("0x0", Some(false)),
            ("0X1f", Some(true)),
            ("0b10", Some(true)),
            ("0o7", Some(true)),
            ("07", Some(true)),
            ("00", Some(false)),
            ("1_000", Some(true)),
            ("0x_1", Some(true)),
            ("08", None),
            ("_1", None),
            ("1_", None),
            ("1__0", None),
            ("0x", None),
            ("", None),
            ("1.5", None),
        ] {
            assert_eq!(go_int_is_nonzero(text), want, "{text:?}");
        }
    }

    #[test]
    fn merge_keys_leave_nodes_without_labels() {
        let dir = std::env::temp_dir().join(format!("clod-proxy-label-merge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("merged.yaml");
        std::fs::write(
            &file,
            "base: &ws {network: ws, ws-opts: {v2ray-http-upgrade: true}}\nproxies:\n  \
             - {name: M, type: vless, tls: true, <<: *ws}\n  - {name: P, type: trojan}\n",
        )
        .unwrap();
        let config = yaml(&format!(
            "proxies:\n  - {{name: A, type: trojan}}\n  - {{name: B, type: vless, tls: true, <<: {{network: grpc}}}}\n\
             proxy-providers:\n  \
             file:\n    type: file\n    path: {}\n  \
             inline:\n    type: inline\n    payload: [{{name: I, type: vless, <<: {{network: ws}}}}, {{name: J, type: ss}}]\n  \
             plain:\n    type: inline\n    payload: [{{name: K, type: ss}}]\n",
            file.display()
        ));
        let (labels, _, _) = build(&config, true);
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(labels.proxies.len(), 1, "{labels:?}");
        assert_eq!(labels.proxies["A"].text, "Trojan RAW (TCP) · TLS");
        assert!(!labels.providers.contains_key("file"), "{labels:?}");
        assert!(!labels.providers.contains_key("inline"), "{labels:?}");
        assert_eq!(labels.providers["plain"]["K"].text, "Shadowsocks");
    }
}
