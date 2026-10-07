//! Защищённый канал клиент ↔ прослойка (протокол c1), клиентская половина.
//!
//! Секрет, из которого выводятся все ключи, — сам токен подписки. В сеть он
//! не уходит никогда: запрос идёт по адресу `/c1/<kid>/<spid>/<blob>`, где
//! `kid` — метка, меняющаяся каждые сутки. Посредник, терминирующий TLS
//! (CDN), не видит ни адреса подписки, ни заголовков, ни тела.
//!
//! Набор примитивов один и не согласуется: X25519 + HKDF-SHA256 +
//! ChaCha20-Poly1305. Согласование алгоритмов — это дыра на понижение,
//! поэтому его нет.
//!
//! Соответствие протоколу проверяется векторами `chan_vectors.json` — теми же
//! самыми, что лежат в прослойке (PHP) и в Android-ядре (Go). Три реализации
//! обязаны сходиться байт в байт, иначе подписка просто не загрузится.

use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use x25519_dalek::{PublicKey, StaticSecret};

/// Тексты ошибок — стабильные машинные метки, а не фразы.
///
/// Их разбирает `error-explanation.ts` на фронте и превращает в человеческий
/// текст на языке интерфейса. Русская фраза из бэкенда попала бы в окно как
/// есть, мимо всех тринадцати переводов.
pub const VERSION: u8 = 1;
const SALT: &[u8] = b"clod-chan-v1";
/// Допустимый разбег часов, секунд. Симметричный: врут обе стороны.
pub const SKEW: i64 = 300;
/// Значение заголовка не в UTF-8 прослойка присылает в base64 с этим префиксом.
const META_B64: &str = "=?b64?";
/// Больше этого ответ подписки не бывает — защита от бесконечного тела.
const MAX_ANSWER: usize = 32 << 20;
/// Запрос дополняется до кратного этому размеру.
///
/// Без выравнивания длина адреса выдаёт длину карточки устройства: модель
/// телефона, версию системы и сам момент, когда они поменялись, — то есть
/// ровно то, что канал прячет. Прослойка поле `pad` не читает вовсе.
const PAD_BLOCK: usize = 512;
/// `,"pad":""` — столько занимает сам ключ в JSON. Дополнить короче нечем,
/// поэтому если до кратности осталось меньше, добирается целый блок.
const PAD_KEY_LEN: usize = 9;
/// Тело отчёта (`op: rep`) дополняется так, чтобы шифротекст с меткой
/// подлинности был кратен этому размеру: в base64url это ровно 4096 знаков.
/// По длине тела иначе читалось бы, сколько у человека узлов и сетей.
const REPORT_PAD_BLOCK: usize = 3072;
/// Метка подлинности ChaCha20-Poly1305.
const TAG_LEN: usize = 16;

/// То, что раньше ехало заголовками запроса открытым текстом.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Fields {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hwid: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub os: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub osv: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ua: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub acc: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub q: String,
}

#[derive(Serialize)]
struct Request<'a> {
    v: u8,
    t: i64,
    n: &'a str,
    /// Служебная операция канала; у запроса подписки её нет вовсе.
    #[serde(skip_serializing_if = "no_op")]
    op: &'a str,
    #[serde(flatten)]
    fields: &'a Fields,
}

/// Запрос подписки идёт без поля `op`: так он байт в байт прежний.
const fn no_op(op: &&str) -> bool {
    op.is_empty()
}

#[derive(Deserialize)]
struct RawAnswer {
    v: u8,
    t: i64,
    n: String,
    /// Код ответа, который в открытом режиме приехал бы снаружи. Снаружи на
    /// защищённом пути всегда 200: иначе посредник читал бы по коду, чем
    /// кончилось дело, — 404 у неизвестной подписки, 502 при обрыве.
    #[serde(default)]
    st: u16,
    sp: String,
    #[serde(default)]
    meta: HashMap<String, Vec<String>>,
    #[serde(default)]
    body: String,
    /// Тело подписки прослойка отдаёт байт в байт, а JSON так не умеет: одного
    /// байта не в UTF-8 хватает, чтобы кодирование не состоялось. В этом
    /// случае тело приезжает сюда.
    #[serde(default)]
    body_b64: String,
}

/// Разобранный ответ прослойки.
#[derive(Debug)]
pub struct Answer {
    /// Заголовки, которые в открытом режиме приехали бы снаружи.
    pub meta: HeaderMap,
    pub body: String,
    /// Код ответа, приехавший внутри шифра.
    pub status: u16,
    /// Текущий ключ прослойки: закрепляется при первом успехе, дальше
    /// участвует в деривации и даёт совершенную прямую секретность.
    pub sp: [u8; 32],
}

/// Состояние одного обмена: живёт от сборки запроса до разбора ответа.
pub struct Session {
    psk: [u8; 32],
    kid: String,
    dh: Vec<u8>,
    eph_pub: [u8; 32],
    secret: StaticSecret,
    nonce: String,
}

/// HKDF-SHA256 на 32 байта.
///
/// Ошибка здесь невозможна (она бывает только на выводе длиннее 255 блоков),
/// но `expect` в этом проекте запрещён линтером, и правильно: молча вернуть
/// нули значило бы шифровать нулевым ключом.
fn hkdf32(ikm: &[u8], salt: &str, info: &[u8]) -> Result<[u8; 32]> {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(Some(salt.as_bytes()), ikm)
        .expand(info, &mut out)
        .map_err(|_| anyhow!("clod-chan-kdf"))?;
    Ok(out)
}

/// Ключ подписки, выведенный из её адреса.
pub fn psk(token: &str) -> Result<[u8; 32]> {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(Some(SALT), token.as_bytes())
        .expand(b"psk", &mut out)
        .map_err(|_| anyhow!("clod-chan-kdf"))?;
    Ok(out)
}

#[must_use]
pub const fn epoch(now: i64) -> i64 {
    now.div_euclid(86400)
}

/// Метка подписки на сутки: посредник не получает стабильного идентификатора.
pub fn kid(psk: &[u8; 32], epoch: i64) -> Result<String> {
    // Полная форма вызова обязательна: `new_from_slice` есть и у `Mac`,
    // и у `KeyInit` из aead, и компилятор без подсказки их не различает.
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(psk).map_err(|_| anyhow!("clod-chan-kdf"))?;
    mac.update(format!("kid|{epoch}").as_bytes());
    Ok(B64.encode(&mac.finalize().into_bytes()[..9]))
}

/// Короткий отпечаток ключа прослойки: им клиент говорит, каким ключом считал DH.
#[must_use]
pub fn spid(public: &[u8; 32]) -> String {
    let digest = Sha256::digest(public);
    B64.encode(digest)[..6].to_string()
}

fn random32() -> Result<[u8; 32]> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("clod-chan-no-entropy: {e}"))?;
    Ok(buf)
}

/// Делит адрес подписки на префикс и токен.
///
/// Токен — последний непустой сегмент пути. Он и есть общий секрет, поэтому
/// остаётся на устройстве, а наружу уходит только префикс.
fn split(base: &str) -> Result<(String, String, String)> {
    let rest = base.split('#').next().unwrap_or_default();
    let (rest, query) = match rest.split_once('?') {
        Some((head, tail)) => (head, tail.to_string()),
        None => (rest, String::new()),
    };
    let rest = rest.trim_end_matches('/');

    if !rest.starts_with("https://") && !rest.starts_with("http://") {
        bail!("clod-chan-bad-url");
    }

    let cut = rest.rfind('/').ok_or_else(|| anyhow!("clod-chan-bad-url"))?;
    let token = &rest[cut + 1..];
    if token.is_empty() || cut < "https://".len() {
        bail!("clod-chan-bad-url");
    }

    Ok((rest[..cut].to_string(), token.to_string(), query))
}

/// Дополняет открытый текст запроса до кратного [`PAD_BLOCK`].
///
/// Поле дописывается в уже собранный JSON, а не в структуру: так порядок полей
/// остаётся тем же, что в тестовых векторах, и не зависит от сериализатора.
fn pad(mut plain: Vec<u8>) -> Vec<u8> {
    let size = plain.len();
    if size < 2 || size.is_multiple_of(PAD_BLOCK) {
        return plain;
    }

    let need = (PAD_BLOCK - (size + PAD_KEY_LEN) % PAD_BLOCK) % PAD_BLOCK;

    plain.truncate(size - 1);
    plain.extend_from_slice(b",\"pad\":\"");
    plain.resize(plain.len() + need, b'.');
    plain.extend_from_slice(b"\"}");

    plain
}

/// Собирает адрес защищённого запроса и состояние сеанса.
pub fn build(base: &str, pinned: Option<[u8; 32]>, fields: &Fields, now: i64) -> Result<(String, Session)> {
    build_op(base, pinned, fields, "", now)
}

/// Адрес отчёта клиента (`op: rep`) и сеанс: тело запечатывает
/// [`Session::seal_report`], ответ разбирает [`Session::open`].
pub fn build_report(base: &str, pinned: Option<[u8; 32]>, fields: &Fields, now: i64) -> Result<(String, Session)> {
    build_op(base, pinned, fields, "rep", now)
}

fn build_op(base: &str, pinned: Option<[u8; 32]>, fields: &Fields, op: &str, now: i64) -> Result<(String, Session)> {
    let (prefix, token, query) = split(base)?;

    let mut fields = fields.clone();
    if fields.q.is_empty() {
        fields.q = query;
    }

    let psk = psk(&token)?;
    let kid = kid(&psk, epoch(now))?;

    let secret = StaticSecret::from(random32()?);
    let eph_pub = PublicKey::from(&secret).to_bytes();

    // Секрет с долгоживущим ключом прослойки участвует, только если клиент
    // этот ключ уже закрепил: на первом контакте его ещё нет.
    let (spid_part, dh) = match pinned {
        Some(sp) => (
            spid(&sp),
            secret.diffie_hellman(&PublicKey::from(sp)).to_bytes().to_vec(),
        ),
        None => ("0".to_string(), Vec::new()),
    };

    let nonce = B64.encode(&random32()?[..16]);
    let plain = pad(serde_json::to_vec(&Request {
        v: VERSION,
        t: now,
        n: &nonce,
        op,
        fields: &fields,
    })?);

    let mut ikm = psk.to_vec();
    ikm.extend_from_slice(&dh);
    let mut info = b"req".to_vec();
    info.extend_from_slice(&eph_pub);

    let cipher = ChaCha20Poly1305::new(Key::from_slice(&hkdf32(&ikm, &kid, &info)?));
    let mut aad = format!("c1{kid}").into_bytes();
    aad.extend_from_slice(&eph_pub);

    let sealed = cipher
        .encrypt(Nonce::from_slice(&[0u8; 12]), Payload { msg: &plain, aad: &aad })
        .map_err(|_| anyhow!("clod-chan-seal"))?;

    let mut blob = eph_pub.to_vec();
    blob.extend_from_slice(&sealed);

    let url = format!("{prefix}/c1/{kid}/{spid_part}/{}", B64.encode(&blob));

    Ok((
        url,
        Session {
            psk,
            kid,
            dh,
            eph_pub,
            secret,
            nonce,
        },
    ))
}

/// Расшифрованный ответ не разобрался: формат прослойки клиенту не знаком.
fn malformed<E>(_: E) -> anyhow::Error {
    anyhow!("clod-chan-version")
}

/// Заголовки панели из ответа. Значение не в UTF-8 едет в base64 с префиксом
/// [`META_B64`] и возвращается байт в байт; заголовок, которого не бывает в HTTP,
/// отбрасывается — так же, как отбросил бы его открытый путь.
fn headers_of(meta: HashMap<String, Vec<String>>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, values) in meta {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        for value in values {
            let raw = match value.strip_prefix(META_B64) {
                Some(encoded) => B64.decode(encoded).ok(),
                None => Some(value.into_bytes()),
            };
            if let Some(value) = raw.and_then(|raw| HeaderValue::from_bytes(&raw).ok()) {
                headers.append(name.clone(), value);
            }
        }
    }
    headers
}

/// Тело отчёта перед шифрованием: длина сжатых данных (4 байта, big-endian),
/// сами данные и нули до кратного [`REPORT_PAD_BLOCK`] вместе с меткой.
fn report_frame(gz: &[u8]) -> Result<Vec<u8>> {
    let len = u32::try_from(gz.len()).map_err(|_| anyhow!("clod-chan-report-too-big"))?;
    let mut plain = len.to_be_bytes().to_vec();
    plain.extend_from_slice(gz);
    let need = (REPORT_PAD_BLOCK - (plain.len() + TAG_LEN) % REPORT_PAD_BLOCK) % REPORT_PAD_BLOCK;
    plain.resize(plain.len() + need, 0);
    Ok(plain)
}

impl Session {
    /// Ключ тела отчёта: тот же материал, что у запроса, своя метка «rep».
    fn report_key(&self) -> Result<[u8; 32]> {
        let mut ikm = self.psk.to_vec();
        ikm.extend_from_slice(&self.dh);
        let mut info = b"rep".to_vec();
        info.extend_from_slice(&self.eph_pub);
        hkdf32(&ikm, &self.kid, &info)
    }

    /// Запечатывает сжатый отчёт в тело POST: base64url без выравнивания.
    pub fn seal_report(&self, gz: &[u8]) -> Result<String> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.report_key()?));
        let mut aad = format!("c1p{}", self.kid).into_bytes();
        aad.extend_from_slice(&self.eph_pub);
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&[0u8; 12]),
                Payload {
                    msg: &report_frame(gz)?,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("clod-chan-seal"))?;
        Ok(B64.encode(sealed))
    }

    /// Разбирает ответ прослойки.
    ///
    /// Любая неудача — ошибка, а не «ну ладно»: профиль, помеченный
    /// защищённым, открытый ответ не принимает никогда.
    /// `wire` — тело ответа как оно пришло: base64url без выравнивания.
    /// Двоичного тела на проводе нет сознательно: текст проходит через любой
    /// CDN и WAF, а двоичное тело на текстовом пути иногда портят.
    pub fn open(&self, wire: &str, now: i64) -> Result<Answer> {
        if wire.len() > MAX_ANSWER {
            bail!("clod-chan-undecryptable");
        }

        let body = B64
            .decode(wire.trim())
            .map_err(|_| anyhow!("clod-chan-undecryptable"))?;
        if body.len() < 48 {
            bail!("clod-chan-undecryptable");
        }

        let mut s_eph = [0u8; 32];
        s_eph.copy_from_slice(&body[..32]);

        let shared = self.secret.diffie_hellman(&PublicKey::from(s_eph)).to_bytes();

        let mut ikm = self.psk.to_vec();
        ikm.extend_from_slice(&shared);
        ikm.extend_from_slice(&self.dh);
        let mut info = b"res".to_vec();
        info.extend_from_slice(&self.eph_pub);

        let cipher = ChaCha20Poly1305::new(Key::from_slice(&hkdf32(&ikm, &self.kid, &info)?));
        let mut aad = format!("c1r{}", self.kid).into_bytes();
        aad.extend_from_slice(&self.eph_pub);
        aad.extend_from_slice(&s_eph);

        let plain = cipher
            .decrypt(
                Nonce::from_slice(&[0u8; 12]),
                Payload {
                    msg: &body[32..],
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("clod-chan-undecryptable"))?;

        // Дальше ответ уже расшифрован, то есть пришёл от прослойки: любой
        // непорядок внутри — несовместимый формат, а не «канала нет».
        let answer: RawAnswer = serde_json::from_slice(&plain).map_err(malformed)?;
        if answer.v != VERSION {
            bail!("clod-chan-version");
        }
        // Эхо метки запроса: ответ обязан быть ответом именно на наш запрос,
        // а не записанным когда-то раньше.
        if answer.n != self.nonce {
            bail!("clod-chan-mismatch");
        }
        if answer.t <= 0 || (now - answer.t).abs() > SKEW {
            bail!("clod-chan-stale");
        }

        let sp_raw = B64.decode(&answer.sp).map_err(malformed)?;
        let sp: [u8; 32] = sp_raw.try_into().map_err(|_| anyhow!("clod-chan-bad-key"))?;

        // Тело не в UTF-8 сюда доезжает отдельным полем. В `String` его не
        // положить дословно, но и не бывает такого у подписки: base64, YAML и
        // JSON — всё текст. Замена битых байтов лучше, чем отказ загрузиться.
        let body = if answer.body_b64.is_empty() {
            answer.body
        } else {
            String::from_utf8_lossy(&B64.decode(&answer.body_b64).map_err(malformed)?).into_owned()
        };

        Ok(Answer {
            meta: headers_of(answer.meta),
            body,
            status: if answer.st == 0 { 200 } else { answer.st },
            sp,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) mod relay {
    //! Прослойка для тестов клиента: зеркало `chan_open` и `chan_seal` из
    //! `lib/chan.php`, без индекса меток и без памяти о метках запроса.

    use super::{
        B64, ChaCha20Poly1305, Key, Nonce, Payload, PublicKey, SKEW, StaticSecret, VERSION, epoch, hkdf32, kid, psk,
        random32,
    };
    use base64::Engine as _;
    use chacha20poly1305::aead::{Aead as _, KeyInit as _};

    /// Узнанный запрос: то, что прослойке нужно для ответа.
    pub struct Opened {
        psk: [u8; 32],
        kid: String,
        dh: Vec<u8>,
        eph_pub: [u8; 32],
        nonce: String,
    }

    pub struct Relay {
        pub token: String,
        /// Ключи прослойки; первый — текущий, он и уезжает клиенту в `sp`.
        pub keys: Vec<StaticSecret>,
    }

    impl Relay {
        pub fn new(token: &str, keys: &[u8]) -> Self {
            Self {
                token: token.into(),
                keys: keys.iter().map(|seed| StaticSecret::from([*seed; 32])).collect(),
            }
        }

        pub fn public(&self, index: usize) -> [u8; 32] {
            PublicKey::from(&self.keys[index]).to_bytes()
        }

        /// `None` — запрос не узнан: прослойка отдаёт его обычному конвейеру.
        pub fn open(&self, path: &str, now: i64) -> Option<Opened> {
            let path = path.split('?').next()?;
            let mut parts = path.rsplitn(4, '/');
            let (blob, spid, label) = (parts.next()?, parts.next()?, parts.next()?);
            if !parts.next()?.ends_with("/c1") {
                return None;
            }

            let psk = psk(&self.token).ok()?;
            let known = (-1..=1).any(|day| kid(&psk, epoch(now) + day).is_ok_and(|kid| kid == label));
            let raw = B64.decode(blob).ok()?;
            if !known || raw.len() < 49 {
                return None;
            }
            let eph_pub: [u8; 32] = raw[..32].try_into().ok()?;

            let dh = if spid == "0" {
                Vec::new()
            } else {
                let key = self
                    .keys
                    .iter()
                    .find(|key| super::spid(&PublicKey::from(*key).to_bytes()) == spid)?;
                key.diffie_hellman(&PublicKey::from(eph_pub)).to_bytes().to_vec()
            };

            let mut ikm = psk.to_vec();
            ikm.extend_from_slice(&dh);
            let mut info = b"req".to_vec();
            info.extend_from_slice(&eph_pub);
            let mut aad = format!("c1{label}").into_bytes();
            aad.extend_from_slice(&eph_pub);
            let plain = ChaCha20Poly1305::new(Key::from_slice(&hkdf32(&ikm, label, &info).ok()?))
                .decrypt(
                    Nonce::from_slice(&[0u8; 12]),
                    Payload {
                        msg: &raw[32..],
                        aad: &aad,
                    },
                )
                .ok()?;

            let request: serde_json::Value = serde_json::from_slice(&plain).ok()?;
            let fresh = request["t"].as_i64().is_some_and(|t| t > 0 && (now - t).abs() <= SKEW);
            let nonce = request["n"].as_str().filter(|n| n.len() == 22)?.to_string();
            if request["v"] != VERSION || !fresh {
                return None;
            }

            Some(Opened {
                psk,
                kid: label.into(),
                dh,
                eph_pub,
                nonce,
            })
        }

        /// Ответ канала: `st` — код ответа внутри шифра.
        pub fn seal(&self, opened: &Opened, meta: serde_json::Value, body: &str, st: u16, now: i64) -> String {
            let payload = serde_json::json!({
                "v": VERSION,
                "t": now,
                "n": opened.nonce,
                "st": st,
                "sp": B64.encode(self.public(0)),
                "meta": meta,
                "body": body,
            });
            self.seal_plain(opened, payload.to_string().as_bytes())
        }

        /// Ответ канала с произвольным содержимым под шифром.
        pub fn seal_plain(&self, opened: &Opened, plain: &[u8]) -> String {
            let secret = StaticSecret::from(random32().unwrap());
            let public = PublicKey::from(&secret).to_bytes();
            let shared = secret.diffie_hellman(&PublicKey::from(opened.eph_pub)).to_bytes();

            let mut ikm = opened.psk.to_vec();
            ikm.extend_from_slice(&shared);
            ikm.extend_from_slice(&opened.dh);
            let mut info = b"res".to_vec();
            info.extend_from_slice(&opened.eph_pub);
            let mut aad = format!("c1r{}", opened.kid).into_bytes();
            aad.extend_from_slice(&opened.eph_pub);
            aad.extend_from_slice(&public);

            let sealed = ChaCha20Poly1305::new(Key::from_slice(&hkdf32(&ikm, &opened.kid, &info).unwrap()))
                .encrypt(Nonce::from_slice(&[0u8; 12]), Payload { msg: plain, aad: &aad })
                .unwrap();
            let mut wire = public.to_vec();
            wire.extend_from_slice(&sealed);
            B64.encode(wire)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("chan_vectors.json");

    fn vectors() -> serde_json::Value {
        serde_json::from_str(VECTORS).unwrap()
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    fn hex(raw: &[u8]) -> String {
        raw.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn derivation_matches_vectors() {
        let v = vectors();
        let token = v["token"].as_str().unwrap();

        assert_eq!(hex(&psk(token).unwrap()), v["psk"].as_str().unwrap());
        assert_eq!(
            kid(&psk(token).unwrap(), v["epoch"].as_i64().unwrap()).unwrap(),
            v["kid"].as_str().unwrap()
        );

        let sp: [u8; 32] = unhex(v["sp_public"].as_str().unwrap()).try_into().unwrap();
        assert_eq!(spid(&sp), v["spid"].as_str().unwrap());

        let eph = StaticSecret::from(<[u8; 32]>::try_from(unhex(v["eph_secret"].as_str().unwrap())).unwrap());
        assert_eq!(hex(PublicKey::from(&eph).as_bytes()), v["eph_public"].as_str().unwrap());
        assert_eq!(
            hex(eph.diffie_hellman(&PublicKey::from(sp)).as_bytes()),
            v["dh"].as_str().unwrap()
        );
    }

    #[test]
    fn request_keys_match_vectors() {
        let v = vectors();
        let token = v["token"].as_str().unwrap();
        let kid_s = v["kid"].as_str().unwrap();
        let eph_pub = unhex(v["eph_public"].as_str().unwrap());
        let dh = unhex(v["dh"].as_str().unwrap());

        let mut info = b"req".to_vec();
        info.extend_from_slice(&eph_pub);

        assert_eq!(
            hex(&hkdf32(&psk(token).unwrap(), kid_s, &info).unwrap()),
            v["request"]["key_first"].as_str().unwrap()
        );

        let mut ikm = psk(token).unwrap().to_vec();
        ikm.extend_from_slice(&dh);
        let key = hkdf32(&ikm, kid_s, &info).unwrap();
        assert_eq!(hex(&key), v["request"]["key_pinned"].as_str().unwrap());

        // И сам шифротекст: nonce нулевой, ключ уникален — вектор воспроизводим.
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        let mut aad = format!("c1{kid_s}").into_bytes();
        aad.extend_from_slice(&eph_pub);
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&[0u8; 12]),
                Payload {
                    msg: v["request"]["plain"].as_str().unwrap().as_bytes(),
                    aad: &aad,
                },
            )
            .unwrap();
        let mut blob = eph_pub.clone();
        blob.extend_from_slice(&sealed);
        assert_eq!(B64.encode(&blob), v["request"]["blob_pinned"].as_str().unwrap());
    }

    fn session_from_vectors(v: &serde_json::Value, nonce: &str) -> Session {
        Session {
            psk: psk(v["token"].as_str().unwrap()).unwrap(),
            kid: v["kid"].as_str().unwrap().to_string(),
            dh: unhex(v["dh"].as_str().unwrap()),
            eph_pub: unhex(v["eph_public"].as_str().unwrap()).try_into().unwrap(),
            secret: StaticSecret::from(<[u8; 32]>::try_from(unhex(v["eph_secret"].as_str().unwrap())).unwrap()),
            nonce: nonce.to_string(),
        }
    }

    #[test]
    fn answer_matches_vectors() {
        let v = vectors();
        let body = v["response"]["body"].as_str().unwrap();
        let nonce = v["response"]["expect"]["nonce"].as_str().unwrap();
        let session = session_from_vectors(&v, nonce);

        // Метку времени берём из вектора: тест не должен зависеть от часов
        // машины, на которой он гоняется.
        let sealed_at = v["response"]["expect"]["t"].as_i64().unwrap();

        let stale = session.open(body, sealed_at + 3600).unwrap_err().to_string();
        assert!(stale.contains("clod-chan-stale"), "{stale}");

        let answer = session.open(body, sealed_at).unwrap();
        assert_eq!(
            answer.meta.get("announce").map(HeaderValue::as_bytes),
            v["response"]["expect"]["meta_announce"].as_str().map(str::as_bytes)
        );
        assert_eq!(answer.body, v["response"]["expect"]["config"].as_str().unwrap());
        assert_eq!(
            u64::from(answer.status),
            v["response"]["expect"]["st"].as_u64().unwrap()
        );
        assert_eq!(hex(&answer.sp), v["sp_public"].as_str().unwrap());
    }

    #[test]
    fn answer_to_another_request_is_refused() {
        let v = vectors();
        let body = v["response"]["body"].as_str().unwrap();
        let session = session_from_vectors(&v, "чужая-метка-запроса");

        let err = session
            .open(body, v["response"]["expect"]["t"].as_i64().unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("clod-chan-mismatch"), "{err}");
    }

    #[test]
    fn request_is_padded_to_a_block() {
        let v = vectors();
        let token = v["token"].as_str().unwrap();
        let block = v["req_pad_block"].as_u64().unwrap() as usize;

        assert_eq!(v["request"]["plain"].as_str().unwrap().len() % block, 0);

        // Длина адреса не должна зависеть от того, что в карточке устройства.
        let base = format!("https://sub.dom/{token}");
        let (short, _) = build(
            &base,
            None,
            &Fields {
                hwid: "a".into(),
                ..Fields::default()
            },
            1786500000,
        )
        .unwrap();
        let (long, _) = build(
            &base,
            None,
            &Fields {
                hwid: "3f9c1d2e-aaaa-bbbb-cccc-ddddddddddddd".into(),
                os: "windows".into(),
                osv: "11".into(),
                model: "полное имя устройства пользователя".into(),
                ua: "ClodClash/0.0.26 (Windows)".into(),
                ..Fields::default()
            },
            1786500000,
        )
        .unwrap();

        assert_eq!(short.len(), long.len(), "длина адреса выдаёт карточку устройства");
    }

    #[test]
    fn nonce_is_sixteen_bytes() {
        let v = vectors();
        let want = v["nonce_len"].as_u64().unwrap() as usize;
        let (_, session) = build(
            &format!("https://sub.dom/{}", v["token"].as_str().unwrap()),
            None,
            &Fields::default(),
            1786500000,
        )
        .unwrap();

        assert_eq!(session.nonce.len(), want);
    }

    #[test]
    fn binary_body_arrives_in_its_own_field() {
        let v = vectors();
        let nonce = v["response"]["expect"]["nonce"].as_str().unwrap();
        let session = session_from_vectors(&v, nonce);

        let answer = session
            .open(
                v["response"]["body_binary"].as_str().unwrap(),
                v["response"]["expect"]["t"].as_i64().unwrap(),
            )
            .unwrap();

        let want = base64::engine::general_purpose::STANDARD
            .decode(v["response"]["expect"]["config_binary"].as_str().unwrap())
            .unwrap();
        assert_eq!(answer.body.as_bytes(), String::from_utf8_lossy(&want).as_bytes());
    }

    #[test]
    fn report_envelope_matches_vectors() {
        let v = vectors();
        let rep = &v["report"];
        let fields = Fields {
            hwid: "3f9c1d2e".into(),
            os: "windows".into(),
            ..Fields::default()
        };
        let plain = pad(serde_json::to_vec(&Request {
            v: VERSION,
            t: 1786500000,
            n: "BAECAwQFBgcICQoLDA0ODw",
            op: "rep",
            fields: &fields,
        })
        .unwrap());
        assert_eq!(std::str::from_utf8(&plain).unwrap(), rep["plain"].as_str().unwrap());

        let session = session_from_vectors(&v, "BAECAwQFBgcICQoLDA0ODw");
        assert_eq!(hex(&session.report_key().unwrap()), rep["key"].as_str().unwrap());

        let gz = base64::engine::general_purpose::STANDARD
            .decode(rep["gzip"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            report_frame(&gz).unwrap().len() as u64,
            rep["frame_len"].as_u64().unwrap()
        );
        let body = session.seal_report(&gz).unwrap();
        assert_eq!(body, rep["body"].as_str().unwrap());
        assert_eq!(body.len() % 4096, 0);
    }

    #[test]
    fn a_subscription_request_carries_no_operation() {
        let plain = serde_json::to_string(&Request {
            v: VERSION,
            t: 1,
            n: "x",
            op: "",
            fields: &Fields::default(),
        })
        .unwrap();
        assert!(!plain.contains("\"op\""), "{plain}");
    }

    #[test]
    fn report_body_length_hides_the_size() {
        let (_, session) =
            build_report("https://sub.dom/a7Kd93mQz1Lp0Xr8", None, &Fields::default(), 1786500000).unwrap();
        let small = session.seal_report(&[1u8; 10]).unwrap();
        let larger = session.seal_report(&[1u8; 2000]).unwrap();
        assert_eq!(small.len(), larger.len());
        assert_eq!(session.seal_report(&[1u8; 3100]).unwrap().len(), 8192);
    }

    /// Запрос клиента, узнанный тестовой прослойкой.
    fn exchange(relay: &relay::Relay, now: i64) -> (Session, relay::Opened) {
        let (url, session) = build(
            &format!("https://sub.example/{}", relay.token),
            None,
            &Fields::default(),
            now,
        )
        .unwrap();
        let opened = relay.open(url.trim_start_matches("https://sub.example"), now).unwrap();
        (session, opened)
    }

    #[test]
    fn a_header_value_not_in_utf8_arrives_byte_for_byte() {
        let relay = relay::Relay::new("a7Kd93mQz1Lp0Xr8", &[7]);
        let (session, opened) = exchange(&relay, 1786500000);
        let meta = serde_json::json!({
            "profile-title": [format!("=?b64?{}", B64.encode([0xcf, 0xf0, 0xe8]))],
            "support-url": ["https://example.com/help"],
        });
        let answer = session
            .open(&relay.seal(&opened, meta, "proxies: []", 200, 1786500000), 1786500000)
            .unwrap();

        assert_eq!(
            answer.meta.get("profile-title").map(HeaderValue::as_bytes),
            Some(&[0xcf, 0xf0, 0xe8][..])
        );
        assert_eq!(
            answer.meta.get("support-url").map(HeaderValue::as_bytes),
            Some(&b"https://example.com/help"[..])
        );
    }

    #[test]
    fn a_decrypted_answer_in_a_foreign_format_is_reported_as_such() {
        let relay = relay::Relay::new("a7Kd93mQz1Lp0Xr8", &[7]);
        for plain in [&b"not json"[..], br#"{"v":1,"t":1786500000,"n":"x","sp":"%%"}"#] {
            let (session, opened) = exchange(&relay, 1786500000);
            let mut plain = plain.to_vec();
            if plain.starts_with(b"{") {
                plain = String::from_utf8(plain)
                    .unwrap()
                    .replace("\"x\"", &format!("{:?}", session.nonce))
                    .into_bytes();
            }
            let err = session
                .open(&relay.seal_plain(&opened, &plain), 1786500000)
                .unwrap_err()
                .to_string();
            assert!(err.contains("clod-chan-version"), "{err}");
        }
    }

    #[test]
    fn split_takes_the_last_segment() {
        for (input, prefix, token) in [
            ("https://sub.dom/abc", "https://sub.dom", "abc"),
            ("https://sub.dom/sub/abc/", "https://sub.dom/sub", "abc"),
            ("https://sub.dom/abc?fmt=yaml", "https://sub.dom", "abc"),
            ("https://sub.dom/abc#c", "https://sub.dom", "abc"),
        ] {
            let (got_prefix, got_token, _) = split(input).unwrap();
            assert_eq!((got_prefix.as_str(), got_token.as_str()), (prefix, token), "{input}");
        }

        assert!(split("https://sub.dom/").is_err());
        assert!(split("ftp://sub.dom/abc").is_err());
    }

    #[test]
    fn built_url_looks_like_a_subscription_path() {
        let (url, session) = build("https://sub.dom/a7Kd93mQz1Lp0Xr8", None, &Fields::default(), 1786500000).unwrap();
        assert!(url.starts_with("https://sub.dom/c1/"), "{url}");
        assert!(url.contains("/0/"), "первый контакт идёт без отпечатка ключа: {url}");
        assert_eq!(session.kid.len(), 12);
        // Токен наружу не уходит ни в каком виде.
        assert!(!url.contains("a7Kd93mQz1Lp0Xr8"));
    }
}
