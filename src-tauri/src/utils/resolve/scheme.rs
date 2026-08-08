use anyhow::Result;
use percent_encoding::percent_decode_str;
use smartstring::alias::String;
use tauri::Url;

use crate::{
    config::{Config, PrfItem, profiles},
    core::{CoreManager, handle, timer::Timer},
    utils::help,
};
use clash_verge_logging::{Type, logging, logging_error};

pub(super) async fn resolve_scheme(param: &str) -> Result<()> {
    let param_str = if param.starts_with("[") && param.len() > 4 {
        param
            .get(2..param.len() - 2)
            .ok_or_else(|| anyhow::anyhow!("Invalid string slice boundaries"))?
    } else {
        param
    };
    let masked_deep_link = help::mask_url(param_str);

    logging!(debug, Type::Config, "received deep link: {masked_deep_link}");

    let link_parsed = Url::parse(param_str)
        .map_err(|e| anyhow::anyhow!("failed to parse deep link: {e:?}, param: {masked_deep_link}"))?;

    let Some((url, name)) = extract_subscription_info(&link_parsed) else {
        // Either there is no `url=` at all, or what stands there is not an
        // http(s) address we are willing to fetch.
        logging!(
            warn,
            Type::Config,
            "no importable http(s) url in deep link: {masked_deep_link}"
        );
        return Ok(());
    };

    import_subscription(&url, name.as_ref()).await;
    Ok(())
}

fn extract_subscription_info(link_parsed: &Url) -> Option<(std::string::String, Option<String>)> {
    // clod: `clodclash` is our own scheme; the upstream ones keep working so
    // links made for other Clash clients still import.
    if !matches!(link_parsed.scheme(), "clash" | "clash-verge" | "clodclash") {
        return None;
    }

    let name = link_parsed
        .query_pairs()
        .find(|(key, _)| key == "name")
        .map(|(_, value)| value.into_owned().into());
    let url = extract_subscription_url(link_parsed)?;
    Some((url, name))
}

fn extract_subscription_url(link_parsed: &Url) -> Option<std::string::String> {
    let query = link_parsed.query()?;
    let prefix = "url=";
    let pos = query.find(prefix)?;
    let raw_url = query[pos + prefix.len()..].trim();
    let decoded = decode_subscription_url(raw_url);
    is_importable_url(&decoded).then_some(decoded)
}

/// clod: по ссылке из системы ходим только на http и https.
///
/// Deep-link приходит от кого угодно: письмо, чужая страница, буфер обмена.
/// `url=file:///…` заставил бы нас прочитать файл с диска и показать его
/// содержимое в ошибке импорта, а `url=clodclash://…` — зациклить обработчик
/// на самого себя. Тот же запрет стоит в разборе адреса подписки; здесь он
/// нужен раньше, чтобы отказ был виден в логе с причиной.
fn is_importable_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some_and(|host| !host.is_empty())
    })
}

fn decode_subscription_url(raw_url: &str) -> std::string::String {
    // Avoid double-decoding nested subscription URLs; decode only when needed.
    if Url::parse(raw_url).is_ok() {
        return raw_url.to_string();
    }

    let mut candidate = raw_url.to_string();
    for _ in 0..2 {
        let next = percent_decode_str(&candidate).decode_utf8_lossy().to_string();
        if next == candidate {
            break;
        }
        candidate = next;
        if Url::parse(&candidate).is_ok() {
            break;
        }
    }
    candidate
}

async fn import_subscription(url: &str, name: Option<&String>) {
    let had_current_profile = {
        let profiles = Config::profiles().await;
        profiles.latest_arc().current.is_some()
    };

    let Some(mut item) = fetch_profile_item(url, name).await else {
        return;
    };

    let uid = item.uid.clone().unwrap_or_default();
    if let Err(e) = profiles::profiles_append_item_safe(&mut item).await {
        logging!(error, Type::Config, "failed to import subscription url: {:?}", e);
        Config::profiles().await.discard();
        handle::Handle::notice_message("import_sub_url::error", e.to_string());
        return;
    }

    if let Err(e) = Config::profiles().await.data_arc().save_file().await {
        logging!(error, Type::Config, "failed to save imported subscription: {}", e);
        handle::Handle::notice_message("import_sub_url::error", e.to_string());
        return;
    }
    logging_error!(Type::Timer, Timer::global().refresh().await);
    handle::Handle::notice_message(
        "import_sub_url::ok",
        "", // передаём пустой msg, не хотим вызывать зацикливание
            // бэкенд-фронтенд-бэкенд, здесь только уведомление.
    );

    post_import_updates(&uid, had_current_profile).await;
}

async fn fetch_profile_item(url: &str, name: Option<&String>) -> Option<PrfItem> {
    match PrfItem::from_url(url, name, None, None).await {
        Ok(item) => Some(item),
        Err(e) => {
            logging!(error, Type::Config, "failed to parse profile from url: {:?}", e);
            handle::Handle::notice_message("import_sub_url::error", e.to_string());
            None
        }
    }
}

async fn post_import_updates(uid: &String, had_current_profile: bool) {
    handle::Handle::refresh_verge();
    handle::Handle::notify_profile_changed(uid);

    let should_update_core = if uid.is_empty() || had_current_profile {
        false
    } else {
        let profiles = Config::profiles().await;
        profiles.latest_arc().is_current_profile_index(uid)
    };

    if should_update_core {
        refresh_core_config().await;
    }
}

async fn refresh_core_config() {
    logging!(
        info,
        Type::Config,
        "Deep link import set current profile; refreshing core config"
    );
    match CoreManager::global().update_config_forced().await {
        Ok(outcome) if outcome.is_valid() => handle::Handle::refresh_clash(),
        Ok(outcome) => {
            let message = outcome.to_string();
            logging!(warn, Type::Config, "Apply config failed: {}", message);
            handle::Handle::notice_message("config_validate::error", message);
        }
        Err(err) => {
            logging!(error, Type::Config, "Apply config error: {}", err);
            handle::Handle::notice_message("update_failed", format!("{err}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extract_subscription_info;
    use tauri::Url;

    fn subscription_url(link: &str) -> Option<std::string::String> {
        let parsed = Url::parse(link).expect("deep link parses");
        extract_subscription_info(&parsed).map(|(url, _)| url)
    }

    #[test]
    fn ordinary_links_still_import() {
        assert_eq!(
            subscription_url("clodclash://install-config?url=https://panel.example/sub/token"),
            Some("https://panel.example/sub/token".into())
        );

        // Процент-кодированный адрес — обычная форма ссылки из письма.
        assert_eq!(
            subscription_url("clash://install-config?url=https%3A%2F%2Fpanel.example%2Fsub"),
            Some("https://panel.example/sub".into())
        );

        // http не запрещаем: панель на голом http это беда пользователя, но
        // рабочая, и ломать импорт таких подписок мы не собирались.
        assert!(subscription_url("clash://install-config?url=http://panel.example/sub").is_some());
    }

    #[test]
    fn foreign_schemes_never_reach_the_fetcher() {
        // Deep-link приходит откуда угодно, и чтение файла с диска или
        // рекурсия по собственной схеме импортом подписки не являются.
        for hostile in [
            "clash://install-config?url=file:///etc/passwd",
            "clash://install-config?url=file%3A%2F%2F%2Fetc%2Fpasswd",
            "clash://install-config?url=javascript:alert(1)",
            "clash://install-config?url=clodclash://install-config?url=x",
            "clash://install-config?url=data:text/yaml;base64,cHJveGllczoge30=",
            // Схема есть, хоста нет — идти некуда.
            "clash://install-config?url=https:///sub",
        ] {
            assert_eq!(subscription_url(hostile), None, "{hostile}");
        }
    }

    #[test]
    fn only_our_own_and_upstream_link_schemes_are_answered() {
        assert!(subscription_url("https://panel.example/?url=https://panel.example/sub").is_none());
    }
}
