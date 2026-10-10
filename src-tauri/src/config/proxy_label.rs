//! Протокол, транспорт и защита узла: «VLESS», «RAW (TCP)», «Reality» —
//! отдельными плашками и одной строкой «VLESS RAW (TCP) · Reality». Транспорта
//! и шифрования ядро не отдаёт (стоковое — тем более), поэтому всё берётся из
//! принятой сборки: её `proxies` и провайдеров узлов — и читается так, как его
//! читает clod-core. Где ядро может понять запись иначе, подписи нет: лучше
//! никакой, чем чужая. Тем же обходом узлы получают и адрес — тип, сервер и
//! порт, по которым их узнаёт отчёт.
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

/// Тип, сервер и порт узла из его записи и чем он отличается от соседей на
/// том же адресе и порту ([`via`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Address {
    /// Тип строчными буквами.
    pub kind: String,
    pub server: String,
    pub port: u16,
    pub via: String,
    /// Провайдер, у которого ядро держит узел; пусто — узел самой подписки.
    pub provider: String,
}

/// Узел, каким его заводит ядро: подпись и адрес — что из них прочлось.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    label: Option<ProxyLabel>,
    address: Option<Address>,
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

/// Адрес узла: тип, сервер и порт 1–65535 (числом или строкой).
fn address(proxy: &Mapping) -> Option<Address> {
    let text = |key: &str| proxy.get(key).and_then(Value::as_str);
    let port = proxy
        .get("port")
        .and_then(|port| {
            port.as_u64()
                .or_else(|| port.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)?;
    let kind = text("type")?.to_ascii_lowercase();
    Some(Address {
        via: via(&kind, proxy),
        kind,
        server: text("server")?.to_owned(),
        port,
        provider: String::new(),
    })
}

/// Строка поля или первая строка списка.
fn first_text(value: Option<&Value>) -> &str {
    match value {
        Some(Value::String(text)) => text.trim(),
        Some(Value::Sequence(list)) => list.first().and_then(Value::as_str).map_or("", str::trim),
        _ => "",
    }
}

/// Поле `key` блока опций `block` узла.
fn inner<'a>(proxy: &'a Mapping, block: &str, key: &str) -> &'a str {
    first_text(
        opt(proxy, block)
            .and_then(Value::as_mapping)
            .and_then(|block| opt(block, key)),
    )
}

/// Заголовок `Host` блока опций `block` узла.
fn host_header<'a>(proxy: &'a Mapping, block: &str) -> &'a str {
    first_text(
        opt(proxy, block)
            .and_then(Value::as_mapping)
            .and_then(|block| opt(block, "headers"))
            .and_then(Value::as_mapping)
            .and_then(|headers| opt(headers, "host")),
    )
}

/// Чем узел отличается от соседей на том же адресе и порту — так панель
/// разводит хосты за одним доменом: транспорт, имя сервера TLS, `Host` и путь
/// (у gRPC — имя сервиса). Пусто, если ничего из этого нет. Правило то же, что
/// у Android (`report.Via`): по этой строке прослойка узнаёт один узел с обеих
/// платформ.
fn via(kind: &str, proxy: &Mapping) -> String {
    // Регистр — только латиница, как у Android и прослойки (strings.ToLower на
    // ASCII-строке, PHP strtolower).
    let network = first_text(opt(proxy, "network")).to_ascii_lowercase();
    let sni = [first_text(opt(proxy, "servername")), first_text(opt(proxy, "sni"))]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (host, path) = match network.as_str() {
        "ws" => (host_header(proxy, "ws-opts"), inner(proxy, "ws-opts", "path")),
        "grpc" => ("", inner(proxy, "grpc-opts", "grpc-service-name")),
        "h2" => (inner(proxy, "h2-opts", "host"), inner(proxy, "h2-opts", "path")),
        "http" => (host_header(proxy, "http-opts"), inner(proxy, "http-opts", "path")),
        "xhttp" => (inner(proxy, "xhttp-opts", "host"), inner(proxy, "xhttp-opts", "path")),
        _ if kind == "ss" => (inner(proxy, "plugin-opts", "host"), inner(proxy, "plugin-opts", "path")),
        _ => ("", ""),
    };
    let network = if network == "tcp" { "" } else { network.as_str() };
    let host = host.to_ascii_lowercase();
    if network.is_empty() && sni.is_empty() && host.is_empty() && path.is_empty() {
        return String::new();
    }
    format!("{network}|{sni}|{host}|{path}")
}

fn node(proxy: &Mapping) -> Node {
    Node {
        label: label(proxy),
        address: address(proxy),
    }
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
    fn apply(&self, proxies: Option<&Value>) -> HashMap<String, Node> {
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
            out.insert(format!("{}{name}{}", self.prefix, self.suffix), node(proxy));
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

/// Что собрано из сборки за один обход.
struct Built {
    labels: Labels,
    /// Адреса узлов по именам, какими их называет ядро: узлы самой подписки
    /// первее провайдерских, провайдеры — по порядку имён.
    addresses: HashMap<String, Address>,
    /// Файлы провайдеров, из которых прочитано, с состоянием до чтения.
    files: Vec<(PathBuf, FileState)>,
    /// Ни один файл не переписывался, пока его читали.
    steady: bool,
}

/// Подписи и адреса узлов сборки. `own_files` — файлы скачанных провайдеров
/// лежат у приложения: ядро запущено им самим, а не службой. У службы они в её
/// папке, закрытой для нас; файл в папке приложения — от прежнего запуска без
/// службы: окну его подписи не показываются, отчёту адреса из него годятся.
fn build(config: &Mapping, own_files: bool) -> Built {
    let mut built = Built {
        labels: Labels::default(),
        addresses: HashMap::new(),
        files: Vec::new(),
        steady: true,
    };
    for proxy in listed(config.get("proxies")) {
        if proxy.contains_key("<<") || proxy.values().any(crate::utils::help::contains_merge_key) {
            continue;
        }
        let Some(name) = proxy.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Node { label, address } = node(proxy);
        if let Some(label) = label {
            built.labels.proxies.entry(name.to_owned()).or_insert(label);
        }
        if let Some(address) = address {
            built.addresses.entry(name.to_owned()).or_insert(address);
        }
    }
    // Провайдеры — по порядку имён, как их отдаёт ядро и как их обходит
    // Android: одноимённый узел двух провайдеров берётся у одного и того же.
    let mut providers: Vec<(String, &Mapping)> = config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(name, provider)| Some((yaml_name(name)?, provider.as_mapping()?)))
        .collect();
    providers.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, provider) in providers {
        let Some(shaping) = Shaping::of(provider) else {
            continue;
        };
        let payload = field(provider, "payload", false);
        if has_merge(payload) {
            continue;
        }
        let kind = field(provider, "type", false).and_then(Value::as_str);
        let labelled = own_files || kind != Some("http");
        let nodes = match kind {
            Some("inline") => shaping.apply(payload),
            Some("file" | "http") => {
                let Some(path) = core_file(kind == Some("http"), provider) else {
                    continue;
                };
                let before = file_state(&path);
                let doc = before
                    .and_then(|_| std::fs::read_to_string(&path).ok())
                    .and_then(|text| serde_yaml_ng::from_str::<Value>(&text).ok());
                built.steady &= file_state(&path) == before;
                built.files.push((path, before));
                let proxies = doc.as_ref().and_then(|doc| doc.get("proxies"));
                if has_merge(proxies) {
                    continue;
                }
                // `payload` у них — запасной набор, пока файл не прочитан ядром.
                // Какой из двух сейчас у ядра, не узнать: где они расходятся,
                // подписи (и адреса) нет.
                let fallback = shaping.apply(payload);
                let mut nodes = shaping.apply(proxies);
                for (name, node) in &mut nodes {
                    let Some(other) = fallback.get(name) else {
                        continue;
                    };
                    if other.label.is_some() && other.label != node.label {
                        node.label = None;
                    }
                    if other.address.is_some() && other.address != node.address {
                        node.address = None;
                    }
                }
                nodes
            }
            _ => continue,
        };
        let mut labels = HashMap::with_capacity(nodes.len());
        for (node_name, Node { label, address }) in nodes {
            if let Some(address) = address {
                built.addresses.entry(node_name.clone()).or_insert_with(|| Address {
                    provider: name.clone(),
                    ..address
                });
            }
            if let Some(label) = label {
                labels.insert(node_name, label);
            }
        }
        if labelled {
            built.labels.providers.insert(name, labels);
        }
    }
    built
}

/// Последние подписи и адреса: по какой сборке, при каком запуске ядра и каких
/// файлах провайдеров собраны. Сборку держим, а не помним адрес: адрес
/// освобождённой мог бы достаться новой.
struct Cached {
    runtime: SharedDraft<IRuntime>,
    own_files: bool,
    files: Vec<(PathBuf, FileState)>,
    stamp: String,
    labels: Arc<Labels>,
    addresses: Arc<HashMap<String, Address>>,
}

static CACHE: Mutex<Option<Arc<Cached>>> = Mutex::new(None);

/// Подписи и адреса принятой сборки. Пересобираются, только когда сменилась
/// сборка, способ запуска ядра или файл провайдера; файлы читаются вне
/// рантайма. `None` — разбор не состоялся.
async fn cached() -> Option<Arc<Cached>> {
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
        let Built {
            labels,
            addresses,
            files,
            steady,
        } = runtime.config.as_ref().map_or_else(
            || Built {
                labels: Labels::default(),
                addresses: HashMap::new(),
                files: Vec::new(),
                steady: true,
            },
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
            addresses: Arc::new(addresses),
        });
        (fresh, steady)
    })
    .await;
    let (fresh, steady) = fresh.ok()?;
    // Файл переписывали, пока читали, — прочитанное не запоминаем.
    if steady {
        *CACHE.lock() = Some(Arc::clone(&fresh));
    }
    Some(fresh)
}

/// Подписи принятой сборки и их отпечаток.
pub async fn current() -> (String, Arc<Labels>) {
    cached().await.map_or_else(
        || (String::new(), Arc::default()),
        |cached| (cached.stamp.clone(), Arc::clone(&cached.labels)),
    )
}

/// Адреса узлов принятой сборки по именам, какими их называет ядро; `None` —
/// ядро работает не на подписке `uid`.
pub async fn addresses(uid: &str) -> Option<Arc<HashMap<String, Address>>> {
    let cached = cached().await?;
    (cached.runtime.profile_uid.as_deref() == Some(uid)).then(|| Arc::clone(&cached.addresses))
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
    use super::{Address, build, go_int_is_nonzero, label, via};
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

    /// Те же записи и строки, что в тесте Android (`report.Via`): по `via`
    /// прослойка узнаёт один узел с обеих платформ.
    #[test]
    fn via_tells_nodes_on_one_address_apart() {
        for (proxy, want) in [
            ("{type: vless, server: a.example.com, port: 443}", ""),
            (
                "{type: vless, network: tcp, servername: Edge.Example.com, reality-opts: {public-key: k}}",
                "|edge.example.com||",
            ),
            (
                "{type: vless, network: ws, tls: true, servername: cdn.example.com, ws-opts: {path: /a?ed=2048, headers: {Host: CDN.example.com}}}",
                "ws|cdn.example.com|cdn.example.com|/a?ed=2048",
            ),
            (
                "{type: trojan, network: grpc, sni: g.example.com, grpc-opts: {grpc-service-name: svc}}",
                "grpc|g.example.com||svc",
            ),
            (
                "{type: vless, network: xhttp, servername: x.example.com, xhttp-opts: {path: /x, host: X.example.com}}",
                "xhttp|x.example.com|x.example.com|/x",
            ),
            (
                "{type: vmess, network: h2, h2-opts: {host: [h.example.com, i.example.com], path: /h}}",
                "h2||h.example.com|/h",
            ),
            (
                "{type: vmess, network: http, http-opts: {path: [/p, /q], headers: {Host: [p.example.com]}}}",
                "http||p.example.com|/p",
            ),
            (
                "{type: ss, plugin: v2ray-plugin, plugin-opts: {mode: websocket, host: s.example.com, path: /s}}",
                "||s.example.com|/s",
            ),
            ("{type: hysteria2, sni: hy.example.com}", "|hy.example.com||"),
            ("{type: vless, network: ws, ws_opts: {path: /u}}", "ws|||/u"),
            ("{type: vless, network: grpc, ws-opts: {path: /ignored}}", "grpc|||"),
        ] {
            let proxy = yaml(proxy);
            let kind = proxy
                .get("type")
                .and_then(serde_yaml_ng::Value::as_str)
                .unwrap_or_default();
            assert_eq!(via(kind, &proxy), want, "{proxy:?}");
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
        let built = build(&config, true);
        let (labels, files, steady) = (built.labels, built.files, built.steady);
        let service = build(&config, false).labels;
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
    fn node_addresses_come_from_the_same_walk_and_names_as_the_labels() {
        let dir = std::env::temp_dir().join(format!("clod-proxy-address-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("provider.yaml");
        std::fs::write(
            &file,
            "proxies:\n  - {name: A, type: ss, server: a.example.com, port: 1}\n  \
             - {name: F, type: vless, server: f.example.com, port: 2}\n  \
             - {name: S, type: trojan, server: z.example.com, port: 9}\n",
        )
        .unwrap();
        let config = yaml(&format!(
            "proxies:\n  - {{name: S, type: snell, server: s.example.com, port: '3'}}\n  \
             - {{name: N, type: vless, server: n.example.com}}\n  \
             - {{name: M, type: vless, server: m.example.com, port: 4, <<: {{network: ws}}}}\n\
             proxy-providers:\n  \
             inline:\n    type: inline\n    exclude-type: ss\n    override: {{additional-prefix: '[i] ', additional-suffix: ' *'}}\n    \
             payload:\n      - {{name: D, type: ss, server: d.example.com, port: 5}}\n      \
             - {{name: D, type: TUIC, server: e.example.com, port: 6}}\n      \
             - {{name: S, type: vless, server: other.example.com, port: 7}}\n  \
             web:\n    type: http\n    path: {}\n    payload: [{{name: F, type: vless, server: old.example.com, port: 2}}]\n  \
             renamed:\n    type: inline\n    override: {{proxy-name: [{{pattern: a, target: b}}]}}\n    \
             payload: [{{name: R, type: vless, server: r.example.com, port: 8}}]\n",
            file.display()
        ));
        let own = build(&config, true).addresses;
        let service = build(&config, false).addresses;
        std::fs::remove_dir_all(&dir).unwrap();

        let at = |kind: &str, server: &str, port, provider: &str| Address {
            kind: kind.into(),
            server: server.into(),
            port,
            via: String::new(),
            provider: provider.into(),
        };
        // Узел без подписи (тип, которого нет в таблице) — с адресом; без порта и
        // с ключом слияния — без.
        assert_eq!(own.get("S"), Some(&at("snell", "s.example.com", 3, "")));
        assert!(!own.contains_key("N") && !own.contains_key("M"), "{own:?}");
        // Имя — с приставками провайдера, исключённый тип не в счёт, тип —
        // строчными; одноимённый с узлом подписки — за подпиской; у узла
        // провайдера — его имя, чтобы история бралась у него же.
        assert_eq!(own.get("[i] D *"), Some(&at("tuic", "e.example.com", 6, "inline")));
        assert!(!own.contains_key("D"), "{own:?}");
        assert_eq!(own.get("[i] S *"), Some(&at("vless", "other.example.com", 7, "inline")));
        // Файл и запасной набор расходятся адресом — какой у ядра, не узнать.
        assert_eq!(own.get("A"), Some(&at("ss", "a.example.com", 1, "web")));
        assert!(!own.contains_key("F"), "{own:?}");
        assert!(!own.contains_key("R"), "{own:?}");
        // Под службой файл скачанного провайдера — от прежнего запуска в папке
        // приложения: отчёту адреса из него годятся (подписей окну — нет).
        assert_eq!(service.get("A"), own.get("A"), "{service:?}");
        assert!(!service.contains_key("F"), "{service:?}");
        assert_eq!(service.get("[i] D *"), own.get("[i] D *"));
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
        let labels = build(&config, true).labels;
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(labels.proxies.len(), 1, "{labels:?}");
        assert_eq!(labels.proxies["A"].text, "Trojan RAW (TCP) · TLS");
        assert!(!labels.providers.contains_key("file"), "{labels:?}");
        assert!(!labels.providers.contains_key("inline"), "{labels:?}");
        assert_eq!(labels.providers["plain"]["K"].text, "Shadowsocks");
    }
}
