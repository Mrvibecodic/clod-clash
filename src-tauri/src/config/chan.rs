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
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use x25519_dalek::{PublicKey, StaticSecret};

pub const VERSION: u8 = 1;
const SALT: &[u8] = b"clod-chan-v1";
/// Допустимый разбег часов, секунд. Симметричный: врут обе стороны.
const SKEW: i64 = 300;
/// Больше этого ответ подписки не бывает — защита от бесконечного тела.
const MAX_ANSWER: usize = 32 << 20;

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
    #[serde(flatten)]
    fields: &'a Fields,
}

#[derive(Deserialize)]
struct RawAnswer {
    v: u8,
    t: i64,
    n: String,
    sp: String,
    #[serde(default)]
    meta: HashMap<String, Vec<String>>,
    #[serde(default)]
    body: String,
}

/// Разобранный ответ прослойки.
#[derive(Debug)]
pub struct Answer {
    /// Заголовки, которые в открытом режиме приехали бы снаружи.
    pub meta: HashMap<String, Vec<String>>,
    pub body: String,
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
        .map_err(|_| anyhow!("clod-chan: HKDF отказал"))?;
    Ok(out)
}

/// Ключ подписки, выведенный из её адреса.
pub fn psk(token: &str) -> Result<[u8; 32]> {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(Some(SALT), token.as_bytes())
        .expand(b"psk", &mut out)
        .map_err(|_| anyhow!("clod-chan: HKDF отказал"))?;
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
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(psk).map_err(|_| anyhow!("clod-chan: HMAC отказал"))?;
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
    getrandom::fill(&mut buf).map_err(|e| anyhow!("нет источника случайности: {e}"))?;
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
        bail!("clod-chan: адрес подписки не http(s)");
    }

    let cut = rest.rfind('/').ok_or_else(|| anyhow!("clod-chan: адрес без токена"))?;
    let token = &rest[cut + 1..];
    if token.is_empty() || cut < "https://".len() {
        bail!("clod-chan: адрес без токена");
    }

    Ok((rest[..cut].to_string(), token.to_string(), query))
}

/// Собирает адрес защищённого запроса и состояние сеанса.
pub fn build(base: &str, pinned: Option<[u8; 32]>, fields: &Fields, now: i64) -> Result<(String, Session)> {
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
        Some(sp) => (spid(&sp), secret.diffie_hellman(&PublicKey::from(sp)).to_bytes().to_vec()),
        None => ("0".to_string(), Vec::new()),
    };

    let nonce = B64.encode(&random32()?[..16]);
    let plain = serde_json::to_vec(&Request {
        v: VERSION,
        t: now,
        n: &nonce,
        fields: &fields,
    })?;

    let mut ikm = psk.to_vec();
    ikm.extend_from_slice(&dh);
    let mut info = b"req".to_vec();
    info.extend_from_slice(&eph_pub);

    let cipher = ChaCha20Poly1305::new(Key::from_slice(&hkdf32(&ikm, &kid, &info)?));
    let mut aad = format!("c1{kid}").into_bytes();
    aad.extend_from_slice(&eph_pub);

    let sealed = cipher
        .encrypt(Nonce::from_slice(&[0u8; 12]), Payload { msg: &plain, aad: &aad })
        .map_err(|_| anyhow!("clod-chan: не удалось зашифровать запрос"))?;

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

impl Session {
    /// Разбирает ответ прослойки.
    ///
    /// Любая неудача — ошибка, а не «ну ладно»: профиль, помеченный
    /// защищённым, открытый ответ не принимает никогда.
    /// `wire` — тело ответа как оно пришло: base64url без выравнивания.
    /// Двоичного тела на проводе нет сознательно: текст проходит через любой
    /// CDN и WAF, а двоичное тело на текстовом пути иногда портят.
    pub fn open(&self, wire: &str, now: i64) -> Result<Answer> {
        if wire.len() > MAX_ANSWER {
            bail!("clod-chan: ответ не похож на наш");
        }

        let body = B64.decode(wire.trim())?;
        if body.len() < 48 {
            bail!("clod-chan: ответ не похож на наш");
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
            .map_err(|_| anyhow!("clod-chan: ответ не расшифровался"))?;

        let answer: RawAnswer = serde_json::from_slice(&plain)?;
        if answer.v != VERSION {
            bail!("clod-chan: незнакомая версия протокола");
        }
        // Эхо метки запроса: ответ обязан быть ответом именно на наш запрос,
        // а не записанным когда-то раньше.
        if answer.n != self.nonce {
            bail!("clod-chan: ответ не на наш запрос");
        }
        if answer.t <= 0 || (now - answer.t).abs() > SKEW {
            bail!("clod-chan: метка времени вне окна");
        }

        let sp_raw = B64.decode(&answer.sp)?;
        let sp: [u8; 32] = sp_raw
            .try_into()
            .map_err(|_| anyhow!("clod-chan: ключ прослойки не тот длины"))?;

        Ok(Answer {
            meta: answer.meta,
            body: answer.body,
            sp,
        })
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
        assert!(stale.contains("метка времени"), "{stale}");

        let answer = session.open(body, sealed_at).unwrap();
        assert_eq!(
            answer.meta["announce"][0],
            v["response"]["expect"]["meta_announce"].as_str().unwrap()
        );
        assert_eq!(answer.body, v["response"]["expect"]["config"].as_str().unwrap());
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
        assert!(err.contains("не на наш запрос"), "{err}");
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
