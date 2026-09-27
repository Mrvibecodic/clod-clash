use crate::{
    cmd,
    config::{Config, PrfItem, PrfOption, profiles::profiles_draft_update_item_safe, sub_headers},
    core::{CoreManager, handle, tray, validate::ValidationOutcome},
    enhance::Sources,
    utils::help::{self, keep_the_clearer_error, mask_err, mask_url},
};
use anyhow::{Context as _, Result, bail};
use clash_verge_logging::{Type, logging, logging_error};
use smartstring::alias::String;

pub async fn toggle_proxy_profile(profile_index: String) {
    // Клик в трее по уже активной подписке: переключать нечего, а перезагрузка ядра
    // стёрла бы историю задержек. Меню перерисовываем — пункт-галочка снял бы отметку сам.
    if Config::profiles()
        .await
        .latest_arc()
        .is_current_profile_index(&profile_index)
    {
        logging_error!(Type::Tray, tray::Tray::global().update_menu().await);
        return;
    }
    logging_error!(
        Type::Config,
        cmd::patch_profiles_config_by_profile_index(profile_index).await
    );
}

pub async fn switch_proxy_node(group_name: &str, proxy_name: &str) {
    let group = handle::Handle::mihomo().get_group_by_name(group_name).await;
    let previous = match group {
        Ok(group) => group.now.filter(|now| now != proxy_name),
        Err(err) => {
            logging!(
                warn,
                Type::Tray,
                "Warning: не удалось узнать прежний узел группы {group_name}, соединения не закрываются: {err}"
            );
            None
        }
    };
    let selected = handle::Handle::mihomo()
        .select_node_for_group(group_name, proxy_name)
        .await;
    if let Err(err) = selected {
        logging!(
            error,
            Type::Tray,
            "Не удалось переключить прокси: {} -> {}, ошибка: {:?}",
            group_name,
            proxy_name,
            err
        );
        let retried = handle::Handle::mihomo()
            .select_node_for_group(group_name, proxy_name)
            .await;
        if let Err(err) = retried {
            logging!(
                error,
                Type::Tray,
                "Переключение прокси окончательно не удалось: {} -> {}, ошибка: {:?}",
                group_name,
                proxy_name,
                err
            );
            return;
        }
        logging!(
            info,
            Type::Tray,
            "Откат переключения прокси успешен: {} -> {}",
            group_name,
            proxy_name
        );
    } else {
        logging!(
            info,
            Type::Tray,
            "Переключение прокси успешно: {} -> {}",
            group_name,
            proxy_name
        );
    }

    // clod:tray-switch — то же самое доделывается и после удавшегося повтора:
    // раньше вторая попытка меняла узел, но соединения прежнего оставались
    // жить, выбор не запоминался, а после перезапуска показывался старый узел.
    if let Some(previous) = previous {
        crate::process::AsyncHandler::spawn(move || async move {
            crate::feat::close_connections_via(&previous).await;
        });
    }
    if let Err(err) = crate::config::profiles::profiles_set_selected_node_safe(group_name, proxy_name).await {
        logging!(
            warn,
            Type::Tray,
            "Warning: не удалось запомнить выбор узла из трея: {err}"
        );
    }
    handle::Handle::refresh_proxy_config();
    let _ = tray::Tray::global().update_menu().await;
}

struct UpdateTarget {
    url: String,
    option: Option<PrfOption>,
    fallback_url: Option<String>,
    fallback_domain: Option<String>,
}

async fn should_update_profile(uid: &String, ignore_auto_update: bool) -> Result<Option<UpdateTarget>> {
    let profiles = Config::profiles().await;
    let profiles = profiles.latest_arc();
    let item = profiles.get_item(uid)?;
    let is_remote = item.itype.as_ref().is_some_and(|s| s == "remote");

    if !is_remote {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] {uid} не является удалённой подпиской, пропускаю обновление"
        );
        Ok(None)
    } else if item.url.is_none() {
        logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] {uid} отсутствует URL, обновление невозможно"
        );
        bail!("failed to get the profile item url");
    } else if !ignore_auto_update && !item.option.as_ref().and_then(|o| o.allow_auto_update).unwrap_or(true) {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] {} автообновление запрещено, пропускаю обновление",
            uid
        );
        Ok(None)
    } else {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] {} является удалённой подпиской, URL: {}",
            uid,
            mask_url(
                item.url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Profile URL is None"))?
            )
        );
        Ok(Some(UpdateTarget {
            url: item.url.clone().ok_or_else(|| anyhow::anyhow!("Profile URL is None"))?,
            option: item.option.clone(),
            fallback_url: item.fallback_url.clone(),
            fallback_domain: item.fallback_domain.clone(),
        }))
    }
}

async fn disarmed_current_profile(uid: &String) -> Option<std::string::String> {
    let file = Config::profiles().await.latest_arc().get_item(uid).ok()?.file.clone()?;
    let data = tokio::fs::read_to_string(crate::utils::dirs::app_profiles_dir().ok()?.join(file.as_str()))
        .await
        .ok()?;
    crate::config::disarmed_profile(&data)
}

/// Чем закончился приём скачанной подписки.
enum Acceptance {
    /// Файл на диске заменён, реестр обновлён; `delivered` — ядро уже работает с
    /// новым конфигом (профиль текущий).
    Accepted { delivered: bool },
    /// Ядро отвергло собранный из неё конфиг — на проверке (файл на диске прежний)
    /// или уже при доставке (файл заменён, ядро осталось на прежнем). Реестр (срок,
    /// трафик, замки панели, отметка загрузки) обновлён как при приёме, у профиля
    /// пометка «не применено».
    Rejected(ValidationOutcome),
    /// Файл принят и заменён, но доставить ядру не удалось (не поднялось, служба
    /// молчит): работает прежний конфиг, у профиля пометка «не применено».
    DeliveryFailed(anyhow::Error),
    /// До проверки не дошло — признак применения занят дольше ожидания или идёт
    /// выход: файл и реестр прежние, пометок нет, загрузку надо повторить скоро.
    Unverified(ValidationOutcome),
    /// Проверка не состоялась (прибита, не запустилась, таймаут): слова ядра нет,
    /// без него файл не заменяем — провал обновления со своим советом человеку;
    /// метаданные панели в реестр всё же попадают.
    Unchecked(ValidationOutcome),
}

/// Принять скачанную подписку: ядро проверяет её ДО того, как она ляжет на диск
/// вместо рабочего файла.
///
/// Тело пишется в файл-кандидат рядом, из реестра-кандидата (этот профиль —
/// текущий, файл — кандидат) собирается конфиг и проверяется ядром — для любого
/// профиля, не только текущего. Отказ — кандидат удалён, рабочий файл не тронут
/// (FlClashX, Prizrak), метаданные панели в реестр всё равно попадают. Приём —
/// прежний файл сохраняется в `<файл>.prev`, кандидат встаёт на его место, реестр
/// обновляется, и если профиль текущий, та же проверенная сборка уходит ядру без
/// второй проверки. Слепок в памяти для отката не нужен: до приёма на диске всё
/// прежнее.
async fn accept_the_download(uid: &String, mut item: PrfItem) -> Result<Acceptance> {
    let disarming = item.device_refused == Some(true);
    if disarming && let Some(disarmed) = disarmed_current_profile(uid).await {
        item.file_data = Some(disarmed.into());
    }
    let Some(body) = item.file_data.take() else {
        bail!("подписка пришла без содержимого");
    };

    let dir = crate::utils::dirs::app_profiles_dir()?;
    let file = Config::profiles().await.data_arc().file_name_for(uid, &item)?;
    let candidate_path = dir.join(format!("{file}.new"));
    help::write_atomic(&candidate_path, body.as_bytes())
        .await
        .with_context(|| format!("failed to write the subscription candidate \"{file}.new\""))?;

    // Реестр-кандидат выводится из принятого уже под признаком применения: за
    // время ожидания профиль могли переименовать, переключить или удалить.
    let sources = {
        let uid = uid.clone();
        let mut item = item.clone();
        let candidate_file: String = format!("{file}.new").into();
        Sources::default().with_profiles_derived(move |accepted| {
            let mut registry = accepted.clone();
            registry.merge_updated_item(&uid, &mut item)?;
            registry.point_item_file_at(&uid, candidate_file)?;
            registry.current = Some(uid);
            Ok(registry)
        })
    };

    let staged = match CoreManager::global().stage_within(sources, ACCEPTANCE_WAIT).await {
        Ok(Ok(staged)) => staged,
        Ok(Err(outcome)) => {
            let _ = tokio::fs::remove_file(&candidate_path).await;
            return refused_before_the_disk(uid, item, outcome).await;
        }
        Err(err) => {
            // Проверка не запустилась (бинарь не нашёлся, антивирус не пустил):
            // слова ядра нет — тот же исход, что у прибитой проверки.
            let _ = tokio::fs::remove_file(&candidate_path).await;
            let outcome = ValidationOutcome::invalid(
                crate::core::validate::ValidationErrorKind::ProcessTerminated,
                format!("{err:#}"),
            );
            return refused_before_the_disk(uid, item, outcome).await;
        }
    };

    let migrate_url = item.migrate_url.clone();
    let request_option = item.option.clone();
    if let Err(err) = promote_and_record(uid, item, &dir, &file, &candidate_path, disarming).await {
        let _ = tokio::fs::remove_file(&candidate_path).await;
        return Err(err);
    }

    let acceptance = if Config::profiles().await.data_arc().is_current_profile_index(uid) {
        deliver_the_accepted(uid, staged).await
    } else {
        drop(staged);
        Acceptance::Accepted { delivered: false }
    };
    // Уже без признака применения: здесь запрос в сеть.
    follow_migration(uid, migrate_url, request_option).await;
    Ok(acceptance)
}

/// Проверка не пропустила кандидата. Слово ядра о конфиге — отказ: реестр
/// получает метаданные панели как при приёме (срок, трафик, замки, отметка
/// загрузки — иначе замок панели «истекал» бы, а срок стоял бы в прошлом), файл
/// остаётся прежним, у профиля пометка «не применено». Проверка прибита или не
/// уложилась — слова ядра нет, файл не заменяем, человеку — совет по виду отказа.
/// «Занято»/«выход» — до проверки не дошло, повторим скоро.
async fn refused_before_the_disk(uid: &String, mut item: PrfItem, outcome: ValidationOutcome) -> Result<Acceptance> {
    match &outcome {
        // Метаданные панели — в реестр и здесь: замок панели, срок и лимит устройств
        // не должны стареть из-за того, что проверку прибили.
        ValidationOutcome::Invalid { kind, .. } if !kind.is_the_cores_verdict() => {
            profiles_draft_update_item_safe(uid, &mut item).await?;
            Ok(Acceptance::Unchecked(outcome))
        }
        ValidationOutcome::Invalid { .. } => {
            logging!(
                warn,
                Type::Config,
                "[Обновление подписки] ядро отвергло новую подписку, рабочий файл не тронут: {}",
                outcome
            );
            profiles_draft_update_item_safe(uid, &mut item).await?;
            mark_not_applied(uid).await;
            Ok(Acceptance::Rejected(outcome))
        }
        ValidationOutcome::Valid | ValidationOutcome::Busy | ValidationOutcome::Skipped { .. } => {
            Ok(Acceptance::Unverified(outcome))
        }
    }
}

/// Прежний файл — в `<файл>.prev`, кандидат — на его место, метаданные — в реестр.
///
/// Под разрешением реестра и с проверкой, что профиль ещё есть: удаление за время
/// проверки не должно оставить файл без записи. Файл на диске есть в каждый
/// момент: копия пишется до замены, замена — одним переименованием. При
/// обезоруживании (панель отказала устройству) копия прежнего файла не пишется, а
/// существующая удаляется: смысл обезоруживания — убрать ключи с диска.
async fn promote_and_record(
    uid: &String,
    item: PrfItem,
    dir: &std::path::Path,
    file: &str,
    candidate_path: &std::path::Path,
    disarming: bool,
) -> Result<()> {
    let target = dir.join(file);
    let spare = dir.join(format!("{file}.prev"));
    let owner = uid.clone();
    let mut item = item;
    Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            profiles
                .get_item(&owner)
                .with_context(|| "подписка удалена, пока шла проверка — файл не заменяем")?;
            if disarming {
                let _ = tokio::fs::remove_file(&spare).await;
            } else if let Ok(previous) = tokio::fs::read(&target).await
                && let Err(err) = help::write_atomic(&spare, &previous).await
            {
                logging!(
                    warn,
                    Type::Config,
                    "Warning: [Обновление подписки] запасная копия прежнего файла не записана: {err:#}"
                );
            }
            help::rename_into_place(candidate_path, &target)
                .await
                .with_context(|| format!("failed to replace the subscription file \"{file}\""))?;
            profiles.update_item(&owner, &mut item).await?;
            Ok((profiles, ()))
        })
        .await?;
    // Прежняя пометка «не применено» относилась к прежнему содержимому.
    if let Err(err) = crate::config::profiles::profiles_mark_not_applied(uid, false).await {
        logging!(
            warn,
            Type::Config,
            "Warning: не удалось снять пометку о непринятом профиле: {err}"
        );
    }
    Ok(())
}

/// Профиль текущий — та же проверенная сборка уходит ядру без второй проверки.
/// Если собранное совпало с работающим, ядро не трогается.
async fn deliver_the_accepted(uid: &String, staged: crate::core::manager::Staged<'_>) -> Acceptance {
    match staged.deliver_unless_unchanged().await {
        Ok(outcome) if outcome.is_valid() => Acceptance::Accepted { delivered: true },
        Ok(outcome) => {
            mark_not_applied(uid).await;
            Acceptance::Rejected(outcome)
        }
        Err(err) => {
            // Ядро о содержимом ничего не сказало (не поднялось, служба молчит,
            // начался выход): файл на диске годный и остаётся, но работает
            // прежний конфиг. На выходе пометку не ставим — она пережила бы
            // перезапуск и врала бы о годной подписке.
            if !handle::Handle::global().is_exiting() {
                mark_not_applied(uid).await;
            }
            Acceptance::DeliveryFailed(err)
        }
    }
}

/// Сколько приём подписки ждёт занятого признака применения. Загрузки идут
/// параллельно и заканчиваются почти одновременно, проверки — по одной; без
/// ожидания вторая и третья подписка теряли бы свою загрузку до следующего тика.
const ACCEPTANCE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

async fn mark_not_applied(uid: &String) {
    if let Err(err) = crate::config::profiles::profiles_mark_not_applied(uid, true).await {
        logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] не удалось пометить профиль как непринятый: {}",
            mask_err(&err.to_string())
        );
    }
    handle::Handle::refresh_profiles();
}

/// Панель попросила перейти на другой адрес подписки — проверить его и запомнить.
async fn follow_migration(uid: &String, migrate_url: Option<String>, request_option: Option<PrfOption>) {
    let Some(candidate) = migrate_url else {
        return;
    };

    let hops = Config::profiles()
        .await
        .latest_arc()
        .get_item(uid)
        .map_or(0, |item| item.migration_hops.unwrap_or(0));
    if hops >= sub_headers::MAX_MIGRATION_HOPS {
        logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] [clod] ignoring migration to {}: {} consecutive hops already followed",
            mask_url(&candidate),
            hops
        );
        return;
    }

    match PrfItem::from_url(&candidate, None, None, request_option.as_ref()).await {
        Ok(_) => match crate::config::profiles::profiles_migrate_url_safe(uid, candidate.clone()).await {
            Ok(()) => {
                logging!(
                    info,
                    Type::Config,
                    "[Обновление подписки] [clod] provider migrated the subscription URL to {}",
                    mask_url(&candidate)
                );
                handle::Handle::notice_message("clod_sub::url_migrated", mask_url(&candidate));
            }
            Err(err) => logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] [clod] failed to persist the migrated subscription URL: {}",
                mask_err(&err.to_string())
            ),
        },
        Err(err) => logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] [clod] candidate URL {} failed verification, keeping the current one: {}",
            mask_url(&candidate),
            mask_err(&err.to_string())
        ),
    }
}

/// Скачанная подписка и уведомление о том, каким путём она пришла: уведомление
/// уходит только после того, как подписку принял ядро.
struct Downloaded {
    item: PrfItem,
    notice: Option<&'static str>,
}

/// Подписка скачана не напрямую, а через прокси (Clash или системный).
pub(crate) const UPDATED_VIA_PROXY: &str = "update_with_clash_proxy";

/// Ступеней лестницы маршрутов на один адрес.
const LADDER_STEPS: u64 = 3;

/// Любой запрос может быть повторён с запасными корнями TLS
/// (`utils/network.rs`, `should_retry_with_static_webpki_roots`).
const TLS_FALLBACK_ATTEMPTS: u64 = 2;

/// Защищённый канал при неудаче повторяет запрос без закрепления ключа прослойки
/// (`config/prfitem.rs`, `fetch_for_profile`) — ради ротации ключа он и заведён.
const SECURE_CHANNEL_ATTEMPTS: u64 = 2;

/// Запас поверх суммы ступеней: фора прямого маршрута в гонке, паузы между
/// попытками и разбор ответа.
const LADDER_SLACK: std::time::Duration = std::time::Duration::from_secs(10);

/// Потолок самого бюджета. Существует только затем, чтобы прибавление к `Instant`
/// не переполнилось: `Instant + Duration::MAX` — паника. Тридцать лет — то же
/// значение, которое tokio берёт в `Instant::far_future` со ссылкой на переполнение
/// на macOS и FreeBSD. Ни одна работающая настройка сюда не упирается.
const BUDGET_CEILING_SECS: u64 = 30 * 365 * 24 * 60 * 60;

/// Сколько времени отводится на ОДИН адрес подписки — основной или запасной.
///
/// Потолка не было вовсе: сумма таймаутов ступеней ничем не ограничивалась, и
/// отменить ожидание было нечем. Бюджет считается от таймаута, который выбрал сам
/// пользователь в карточке профиля, и от числа законных попыток внутри ступени,
/// поэтому ни один работавший путь не укорачивается: обрывается только зависание
/// сверх того, что лестница может занять честно.
///
/// У каждого адреса бюджет свой — иначе основной адрес съедал бы весь потолок и до
/// запасного домена, ради которого он и заведён, дело не доходило бы никогда. Общего
/// потолка на всё обновление поэтому нет: бюджет режет зависание отдельного адреса,
/// а не суммарное время.
///
/// Приём профиля (`accept_the_download`) идёт вне бюджета, поэтому принятый
/// профиль не может оборваться на середине применения.
fn address_budget(option: Option<&PrfOption>) -> std::time::Duration {
    let timeout = option.and_then(|o| o.timeout_seconds).unwrap_or(20);
    let secure = option.is_some_and(|o| o.secure.unwrap_or(false));

    let attempts_per_step = TLS_FALLBACK_ATTEMPTS * if secure { SECURE_CHANNEL_ATTEMPTS } else { 1 };
    let seconds = timeout.saturating_mul(LADDER_STEPS).saturating_mul(attempts_per_step);

    std::time::Duration::from_secs(seconds.min(BUDGET_CEILING_SECS)).saturating_add(LADDER_SLACK)
}

async fn within_budget<F>(deadline: tokio::time::Instant, work: F) -> Result<PrfItem>
where
    F: std::future::Future<Output = Result<PrfItem>>,
{
    // Бюджет уже вышел — запрос не отправляем вовсе, чтобы не дёргать панель
    // соединением, которое всё равно будет оборвано.
    if tokio::time::Instant::now() >= deadline {
        bail!("clod-sub-budget: на этот адрес подписки отведённое время уже вышло");
    }

    match tokio::time::timeout_at(deadline, Box::pin(work)).await {
        Ok(result) => result,
        Err(_) => bail!("clod-sub-budget: адрес подписки не ответил за отведённое время"),
    }
}

async fn perform_profile_update(
    uid: &String,
    url: &String,
    opt: Option<&PrfOption>,
    option: Option<&PrfOption>,
    fallback_url: Option<String>,
    fallback_domain: Option<String>,
) -> Result<Downloaded> {
    logging!(
        info,
        Type::Config,
        "[Обновление подписки] Начинаю загрузку нового содержимого подписки"
    );
    let mut merged_opt = PrfOption::merge(opt, option);
    let budget = address_budget(merged_opt.as_ref());
    let deadline = tokio::time::Instant::now() + budget;
    let profiles = Config::profiles().await;
    let profiles_arc = profiles.latest_arc();
    let profile_name = profiles_arc
        .get_name_by_uid(uid)
        .unwrap_or_else(|| String::from("UnKnown Profile"));

    let mut last_err;

    match within_budget(deadline, PrfItem::from_url(url, None, None, merged_opt.as_ref())).await {
        Ok(item) => {
            logging!(info, Type::Config, "[Обновление подписки] Подписка скачана");
            return Ok(Downloaded { item, notice: None });
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] Обычное обновление не удалось: {}, пробую обновить через прокси Clash",
                mask_err(&err.to_string())
            );
            last_err = err;
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(true);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(false);

    match within_budget(deadline, PrfItem::from_url(url, None, None, merged_opt.as_ref())).await {
        Ok(item) => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] Подписка скачана через прокси Clash"
            );
            drop(last_err);
            return Ok(Downloaded {
                item,
                notice: Some(UPDATED_VIA_PROXY),
            });
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] Обновление через прокси Clash не удалось: {}, пробую обновить через системный прокси",
                mask_err(&err.to_string())
            );
            last_err = keep_the_clearer_error(last_err, err);
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(false);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(true);

    match within_budget(deadline, PrfItem::from_url(url, None, None, merged_opt.as_ref())).await {
        Ok(item) => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] Подписка скачана через системный прокси"
            );
            drop(last_err);
            return Ok(Downloaded {
                item,
                notice: Some(UPDATED_VIA_PROXY),
            });
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] Обновление через системный прокси не удалось: {}, все попытки исчерпаны",
                mask_err(&err.to_string())
            );
            last_err = keep_the_clearer_error(last_err, err);
        }
    }

    let spare_addresses = [
        fallback_url.filter(|value| !value.trim().is_empty()),
        fallback_domain
            .filter(|value| !value.trim().is_empty())
            .and_then(|domain| sub_headers::swap_domain(url, &domain)),
    ];

    for spare in spare_addresses.into_iter().flatten() {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] [clod] primary URL failed, trying the provider spare address {}",
            mask_url(&spare)
        );

        // У запасного адреса свой бюджет: он существует ровно для того случая,
        // когда основной адрес молчит до последней секунды.
        let spare_deadline = tokio::time::Instant::now() + budget;

        match within_budget(
            spare_deadline,
            PrfItem::from_url_with_ladder(&spare, None, None, merged_opt.as_ref()),
        )
        .await
        {
            Ok(mut item) => {
                item.from_fallback = Some(true);
                drop(last_err);
                return Ok(Downloaded {
                    item,
                    notice: Some("clod_sub::fallback_used"),
                });
            }
            Err(err) => {
                logging!(
                    warn,
                    Type::Config,
                    "Warning: [Обновление подписки] [clod] spare address failed as well: {}",
                    mask_err(&err.to_string())
                );
                last_err = keep_the_clearer_error(last_err, err);
            }
        }
    }

    let last_err = mask_err(&last_err.to_string());
    bail!("{profile_name} - {last_err}")
}

/// Текст отказа, пригодный для показа пользователю.
///
/// `mask_err` прячет адреса подписки, но не трогает путей файловой системы, а в
/// сообщении ядра приезжает полный путь — вместе с именем пользователя ОС. Гоняем
/// его через ту же чистку, что и ответы команд, чтобы в тост не уезжало лишнее.
fn public_failure_text(raw: &str) -> String {
    let masked = mask_err(raw);
    let home = crate::utils::redact::home_prefix();
    String::from(crate::utils::redact::redact(&crate::utils::redact::scrub_home(
        masked.as_str(),
        home.as_deref(),
    )))
}

/// Сообщить о провале фонового обновления.
///
/// Только про текущий профиль: расписание догоняет пропущенные задания пачкой, и
/// при выключенной сети человек получил бы столько красных тостов, сколько у него
/// подписок. О фоновых провалах остальных говорит пометка на их карточках.
async fn announce_the_failure(uid: &String, status: &str, raw: &str) {
    let is_current = Config::profiles().await.latest_arc().is_current_profile_index(uid);
    if !is_current {
        return;
    }

    handle::Handle::notice_message(status, public_failure_text(raw));
}

/// Окно лимита устройств — про ту подписку, которую только что добавили или
/// обновили, и с её именем: иначе отказ фоновой подписки выглядел отказом текущей.
pub async fn announce_device_refusal(uid: &String) {
    let payload = {
        let profiles = Config::profiles().await;
        let profiles = profiles.latest_arc();
        let Ok(item) = profiles.get_item(uid) else {
            return;
        };
        let Some(state) = item
            .hwid_state
            .as_deref()
            .filter(|state| matches!(*state, "limit" | "not_supported"))
        else {
            return;
        };
        serde_json::json!({
            "state": state,
            "supportUrl": item.support_url.as_deref(),
            "name": item.display_name(),
        })
    };
    handle::Handle::hwid_notice(payload);
}

async fn mark_the_update(uid: &String, failed: bool) {
    match crate::config::profiles::profiles_mark_update_failed(uid, failed).await {
        Ok(true) => handle::Handle::refresh_profiles(),
        Ok(false) => {}
        Err(err) => logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] не удалось записать исход обновления: {}",
            mask_err(&err.to_string())
        ),
    }
}

/// Под каким видом отказа показать сообщение.
///
/// Ядро уже различает, что именно пошло не так, и на каждый вид в приложении
/// написан свой перевод с готовым советом — от «антивирус прервал проверку» до
/// «ошибка в скрипте». На пути обновления подписки этот разбор до сих пор
/// выбрасывался, и всё сводилось к одной общей фразе.
const fn failure_notice_status(result: &Result<ValidationOutcome>) -> &'static str {
    match result {
        Ok(ValidationOutcome::Invalid { kind, .. }) => {
            crate::cmd::validate::notice_key(*kind, crate::cmd::validate::ValidationNoticeTarget::Runtime)
        }
        _ => "update_failed",
    }
}

/// Прибраться после того, как ядро приняло новый конфиг.
fn settle_after_a_successful_update(uid: &String) {
    // Пометку «скачано, но не применено» снимает сам путь применения конфига
    // (`core/manager/config.rs`) — там она снимается на всех путях сразу, включая
    // переключение профиля и ручную пересборку.
    logging!(info, Type::Config, "[Обновление подписки] Обновление успешно");
    handle::Handle::refresh_clash();
    if let Err(err) = crate::config::profiles::activate_selected_nodes() {
        logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] restore selection failed: {err}"
        );
    }
    crate::process::AsyncHandler::spawn(|| async {
        crate::module::sub_watcher::run_check().await;
    });
    let logo_uid = uid.clone();
    crate::process::AsyncHandler::spawn(move || async move {
        crate::module::logo_cache::sync(&logo_uid).await;
    });
}

/// Кто запустил обновление подписки — от этого зависит, кому и как сообщать о провале.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTrigger {
    /// Человек, кнопкой: ошибка вернётся ответом команды, тост не нужен.
    Manual,
    /// Расписание: о провале говорит тост (только у текущей подписки).
    Scheduled,
    /// Повтор по расписанию после провала (загрузка после истечения срока идёт
    /// с бэкоффом без сдачи): о первом провале серии уже сказано, об остальных —
    /// только в журнал, иначе тосты шли бы вечно раз в 5 ч.
    ScheduledRetry,
}

impl UpdateTrigger {
    const fn is_manual(self) -> bool {
        matches!(self, Self::Manual)
    }

    const fn announces_failure(self) -> bool {
        matches!(self, Self::Scheduled)
    }
}

/// Кнопка на профиле, которому нечего скачивать (локальный или автообновление
/// запрещено): пересобрать конфиг ядра из того, что уже лежит на диске.
async fn reapply_the_current_profile(uid: &String, trigger: UpdateTrigger) -> Result<()> {
    match CoreManager::global().update_config_with_force(true).await {
        Ok(outcome) if outcome.is_valid() => {
            settle_after_a_successful_update(uid);
            Ok(())
        }
        result => {
            let status = failure_notice_status(&result);
            let message = match result {
                Ok(outcome) => outcome.to_string(),
                Err(err) => err.to_string(),
            };
            let message = public_failure_text(&message);
            logging!(
                error,
                Type::Config,
                "[Обновление подписки] Пересборка не удалась: {}",
                message
            );
            if trigger.announces_failure() {
                announce_the_failure(uid, status, &message).await;
            }
            bail!(message);
        }
    }
}

/// Провал обновления: в журнал, тостом (если положено) и ошибкой вызывающему.
async fn failed(uid: &String, status: &str, what: &str, raw: &str, trigger: UpdateTrigger) -> anyhow::Error {
    let message = public_failure_text(raw);
    logging!(error, Type::Config, "[Обновление подписки] {what}: {message}");
    if trigger.announces_failure() {
        announce_the_failure(uid, status, &message).await;
    }
    anyhow::anyhow!(message)
}

/// Загрузка удалась — это записано, расписание и окно лимита устройств об этом знают.
async fn note_the_download(uid: &String) {
    mark_the_update(uid, false).await;
    logging_error!(Type::Timer, crate::core::Timer::global().refresh().await);
    announce_device_refusal(uid).await;
}

/// Скачанная подписка принята или отвергнута — сказать об этом тем, кому положено.
async fn settle_the_download(uid: &String, downloaded: Downloaded, trigger: UpdateTrigger) -> Result<UpdateOutcome> {
    let Downloaded { item, notice } = downloaded;
    let profile_name = Config::profiles()
        .await
        .data_arc()
        .get_name_by_uid(uid)
        .unwrap_or_else(|| String::from("UnKnown Profile"));
    let acceptance = match Box::pin(accept_the_download(uid, item)).await {
        Ok(acceptance) => acceptance,
        Err(err) => {
            mark_the_update(uid, true).await;
            return Err(failed(uid, "update_failed", "Обновление не удалось", &err.to_string(), trigger).await);
        }
    };

    let delivered = match acceptance {
        Acceptance::Accepted { delivered } => delivered,
        Acceptance::Unverified(outcome) => {
            // Загрузка удалась — это записано; до проверки просто не дошло.
            mark_the_update(uid, false).await;
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] Приём подписки отложен: {}",
                outcome
            );
            if trigger.is_manual() {
                let text = if matches!(outcome, ValidationOutcome::Skipped { .. }) {
                    clash_verge_i18n::t!("common.exitInProgress")
                } else {
                    clash_verge_i18n::t!("common.configApplying")
                };
                bail!("{text}");
            }
            return Ok(UpdateOutcome::RetrySoon);
        }
        Acceptance::Rejected(outcome) => {
            // Загрузка удалась — провалился приём: об этом говорит пометка
            // «не применено», а не «обновление не удалось».
            note_the_download(uid).await;
            let status = failure_notice_status(&Ok(outcome.clone()));
            return Err(failed(uid, status, "Ядро отвергло подписку", &outcome.to_string(), trigger).await);
        }
        Acceptance::Unchecked(outcome) => {
            // Скачано, но не проверено: файл прежний, обновления не случилось;
            // расписание и окно лимита устройств про загрузку всё же узнают.
            mark_the_update(uid, true).await;
            logging_error!(Type::Timer, crate::core::Timer::global().refresh().await);
            announce_device_refusal(uid).await;
            let status = failure_notice_status(&Ok(outcome.clone()));
            return Err(failed(
                uid,
                status,
                "Проверка подписки не состоялась",
                &outcome.to_string(),
                trigger,
            )
            .await);
        }
        Acceptance::DeliveryFailed(err) => {
            // Файл и реестр уже новые: загрузка удалась, не удалась доставка.
            note_the_download(uid).await;
            return Err(failed(
                uid,
                "update_failed",
                "Подписка принята, но ядру не доставлена",
                &err.to_string(),
                trigger,
            )
            .await);
        }
    };

    note_the_download(uid).await;
    if let Some(notice) = notice {
        handle::Handle::notice_message(notice, profile_name);
    }
    if delivered {
        settle_after_a_successful_update(uid);
    }
    Ok(UpdateOutcome::Done)
}

/// Чем закончился запуск обновления — для расписания.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// Сделано (или скачивать было нечего): дальше по расписанию.
    Done,
    /// До проверки не дошло (уже обновляется, признак применения занят): повторить
    /// скоро, а не через интервал, и не считать провалом загрузки.
    RetrySoon,
}

/// Подписки, которые обновляются прямо сейчас — один вход у кнопки и у расписания,
/// поэтому и признак «идёт» живёт здесь, а не в планировщике.
static UPDATES_IN_FLIGHT: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashSet::new()));

/// Одновременных загрузок подписок по расписанию — не больше стольких:
/// просроченные на старте идут очередью, а не залпом лестниц к панели.
const PARALLEL_DOWNLOADS: usize = 3;
static DOWNLOAD_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(PARALLEL_DOWNLOADS);

struct UpdateClaim(String);

impl Drop for UpdateClaim {
    fn drop(&mut self) {
        UPDATES_IN_FLIGHT.lock().remove(&self.0);
    }
}

fn claim_update(uid: &String) -> Option<UpdateClaim> {
    UPDATES_IN_FLIGHT
        .lock()
        .insert(uid.clone())
        .then(|| UpdateClaim(uid.clone()))
}

pub async fn update_profile(
    uid: &String,
    option: Option<&PrfOption>,
    ignore_auto_update: bool,
    trigger: UpdateTrigger,
) -> Result<UpdateOutcome> {
    let Some(_claim) = claim_update(uid) else {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] {uid} уже обновляется, повторный запуск ({trigger:?}) отклонён"
        );
        if trigger.is_manual() {
            bail!("{}", clash_verge_i18n::t!("common.updateInProgress"));
        }
        return Ok(UpdateOutcome::RetrySoon);
    };
    logging!(
        info,
        Type::Config,
        "[Обновление подписки] Начинаю обновление подписки {}",
        uid
    );
    let url_opt = match should_update_profile(uid, ignore_auto_update).await {
        Ok(target) => target,
        Err(err) => {
            mark_the_update(uid, true).await;
            // Ручной вызов покажет ошибку сам — она вернётся ответом команды.
            if trigger.announces_failure() {
                announce_the_failure(uid, "update_failed", &err.to_string()).await;
            }
            return Err(err);
        }
    };

    let Some(target) = url_opt else {
        // Скачивать нечего (локальный профиль или запрет автообновления): кнопка на
        // текущем профиле пересобирает конфиг, расписанию делать нечего.
        if trigger.is_manual() && Config::profiles().await.data_arc().is_current_profile_index(uid) {
            reapply_the_current_profile(uid, trigger).await?;
        }
        return Ok(UpdateOutcome::Done);
    };

    let downloaded = {
        // Очередь загрузок — для расписания: кнопка человека не должна стоять за
        // залпом просроченных подписок.
        let _slot = if trigger.is_manual() {
            None
        } else {
            Some(DOWNLOAD_SLOTS.acquire().await)
        };
        Box::pin(perform_profile_update(
            uid,
            &target.url,
            target.option.as_ref(),
            option,
            target.fallback_url,
            target.fallback_domain,
        ))
        .await
    };
    let downloaded = match downloaded {
        Ok(downloaded) => downloaded,
        Err(err) => {
            release_stale_panel_locks().await;
            mark_the_update(uid, true).await;
            // Загрузка провалилась. Ручной вызов покажет ошибку сам — она
            // уедет наверх и вернётся в интерфейс ответом команды; а вот
            // автообновление до этой правки не сообщало о провале никак:
            // расписание только писало в журнал.
            if trigger.announces_failure() {
                announce_the_failure(uid, "update_failed", &err.to_string()).await;
            }
            return Err(err);
        }
    };

    Box::pin(settle_the_download(uid, downloaded, trigger)).await
}

pub async fn enhance_profiles() -> Result<ValidationOutcome> {
    let outcome = CoreManager::global().update_config_forced().await?;
    if outcome.is_valid() {
        handle::Handle::refresh_clash();
        if let Err(err) = crate::config::profiles::activate_selected_nodes() {
            logging!(
                warn,
                Type::Config,
                "Warning: restore selection after reapply failed: {err}"
            );
        }
    }
    Ok(outcome)
}

const LOCK_GRACE_SECS: i64 = 72 * 60 * 60;

const LOCK_GRACE_INTERVALS: u64 = 3;

fn lock_grace_secs(item: &PrfItem) -> i64 {
    let interval_minutes = item.option.as_ref().and_then(|opt| opt.update_interval).unwrap_or(0);
    let by_interval = interval_minutes
        .saturating_mul(60)
        .saturating_mul(LOCK_GRACE_INTERVALS)
        .min(i64::MAX as u64) as i64;
    by_interval.max(LOCK_GRACE_SECS)
}

fn lock_expired(item: &PrfItem, now: i64) -> bool {
    if item.lock_mode != Some(true) || item.lock_permanent == Some(true) {
        return false;
    }
    let Some(updated) = item.updated.filter(|value| *value > 0) else {
        return false;
    };
    now.saturating_sub(updated as i64) > lock_grace_secs(item)
}

pub async fn release_stale_panel_locks() {
    let now = chrono::Local::now().timestamp();

    let stale: Vec<String> = {
        let profiles = Config::profiles().await.latest_arc();
        profiles
            .items
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|item| lock_expired(item, now))
            .filter_map(|item| item.uid.clone())
            .collect()
    };

    if stale.is_empty() {
        return;
    }

    let released = Config::profiles()
        .await
        .with_data_modify(move |mut profiles| async move {
            let mut released = Vec::new();
            for item in profiles.items.as_mut().into_iter().flatten() {
                let Some(uid) = item.uid.clone() else { continue };
                if stale.contains(&uid) && lock_expired(item, now) {
                    item.lock_mode = None;
                    item.lock_permanent = None;
                    released.push(uid);
                }
            }
            if !released.is_empty() {
                profiles.save_file().await?;
            }
            Ok((profiles, released))
        })
        .await;

    match released {
        Ok(released) if !released.is_empty() => {
            logging!(
                info,
                Type::Config,
                "[clod] panel lock released on {} profile(s): `clod-lock-mode` was not confirmed within the grace period",
                released.len()
            );
            for uid in released {
                handle::Handle::notify_profile_changed(&uid);
            }
            let _ = tray::Tray::global().update_menu().await;
        }
        Ok(_) => {}
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [clod] failed to release stale lock: {err}"
            );
        }
    }
}

#[cfg(test)]
mod lock_expiry_tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60;

    fn locked_item(updated: i64, interval_minutes: Option<u64>) -> PrfItem {
        PrfItem {
            uid: Some("Rtest".into()),
            itype: Some("remote".into()),
            lock_mode: Some(true),
            updated: Some(updated as usize),
            option: interval_minutes.map(|update_interval| PrfOption {
                update_interval: Some(update_interval),
                ..PrfOption::default()
            }),
            ..PrfItem::default()
        }
    }

    #[test]
    fn fresh_lock_stays() {
        let now = 10 * DAY;
        assert!(!lock_expired(&locked_item(now - DAY, None), now));
    }

    #[test]
    fn silent_panel_releases_the_lock() {
        let now = 10 * DAY;
        assert!(lock_expired(&locked_item(now - 4 * DAY, None), now));
    }

    #[test]
    fn a_long_update_interval_stretches_the_grace() {
        let now = 100 * DAY;
        let weekly = 7 * 24 * 60;
        assert!(!lock_expired(&locked_item(now - 4 * DAY, Some(weekly)), now));
        assert!(lock_expired(&locked_item(now - 22 * DAY, Some(weekly)), now));
    }

    #[test]
    fn a_permanent_lock_never_expires() {
        let now = 1000 * DAY;
        let mut permanent = locked_item(now - 900 * DAY, None);
        permanent.lock_permanent = Some(true);
        assert!(!lock_expired(&permanent, now));

        permanent.lock_permanent = None;
        assert!(lock_expired(&permanent, now));
    }

    #[test]
    fn only_a_real_lock_expires() {
        let now = 10 * DAY;
        let mut unlocked = locked_item(now - 100 * DAY, None);
        unlocked.lock_mode = None;
        assert!(!lock_expired(&unlocked, now));

        let mut without_timestamp = locked_item(0, None);
        without_timestamp.updated = None;
        assert!(!lock_expired(&without_timestamp, now));
    }
}

#[cfg(test)]
mod failure_visibility_tests {
    use super::{failure_notice_status, public_failure_text};
    use crate::core::validate::{ValidationErrorKind, ValidationOutcome};

    #[test]
    fn each_kind_of_refusal_keeps_its_own_advice() {
        // На каждый вид отказа в приложении написан свой перевод с готовым советом,
        // и до этой правки все они на пути обновления сводились к одной общей фразе.
        assert_eq!(
            failure_notice_status(&Ok(ValidationOutcome::invalid(
                ValidationErrorKind::ProcessTerminated,
                "Процесс проверки был прерван"
            ))),
            "config_validate::process_terminated"
        );
        assert_eq!(
            failure_notice_status(&Ok(ValidationOutcome::invalid(
                ValidationErrorKind::ScriptSyntax,
                "script syntax error"
            ))),
            "config_validate::script_syntax_error"
        );
        assert_eq!(
            failure_notice_status(&Ok(ValidationOutcome::invalid(
                ValidationErrorKind::CoreRejected,
                "Parse config error"
            ))),
            "config_validate::error"
        );
    }

    #[test]
    fn a_failure_the_core_did_not_judge_stays_a_plain_update_failure() {
        assert_eq!(
            failure_notice_status(&Err(anyhow::anyhow!("не записался файл"))),
            "update_failed"
        );
        assert_eq!(failure_notice_status(&Ok(ValidationOutcome::Busy)), "update_failed");
    }

    #[test]
    fn the_notice_carries_neither_the_address_nor_a_short_token() {
        // Длинный сегмент прячет ещё `mask_err`, короткий — только `redact`:
        // без него `/s/ab12cd` уезжал бы в тост целиком.
        for raw in [
            "failed to fetch https://panel.example/sub/SECRET-TOKEN-VALUE-1234 while reading",
            "failed to fetch https://panel.example/s/ab12cd while reading",
            "request failed: authorization: Bearer ab12cd",
        ] {
            let shown = public_failure_text(raw);
            assert!(!shown.contains("ab12cd"), "{shown}");
            assert!(!shown.contains("SECRET-TOKEN-VALUE-1234"), "{shown}");
        }
    }

    #[test]
    fn the_notice_does_not_carry_the_os_user_name() {
        // `scrub_home` работает от переменных окружения, поэтому проверяем его
        // напрямую: в тесте домашний каталог тот же, что у приложения.
        let Some(home) = crate::utils::redact::home_prefix() else {
            return;
        };

        let raw = std::format!("failed to read {home}/.config/clod/profiles.yaml");
        let shown = public_failure_text(raw.as_str());

        assert!(!shown.contains(home.as_str()), "{shown}");
        assert!(shown.contains('~'), "{shown}");
    }
}

#[cfg(test)]
mod update_budget_tests {
    use super::{LADDER_SLACK, PrfOption, address_budget, keep_the_clearer_error};
    use std::time::Duration;

    fn option(timeout: Option<u64>, secure: Option<bool>) -> PrfOption {
        PrfOption {
            timeout_seconds: timeout,
            secure,
            ..PrfOption::default()
        }
    }

    #[test]
    fn the_default_ladder_fits_into_its_budget() {
        // Три ступени по 20 с, каждая с возможным повтором на запасных корнях TLS.
        assert_eq!(address_budget(None), Duration::from_secs(120) + LADDER_SLACK);
        assert_eq!(
            address_budget(Some(&option(Some(20), None))),
            Duration::from_secs(120) + LADDER_SLACK
        );
    }

    #[test]
    fn the_secure_channel_gets_its_second_attempt() {
        // Защищённый канал повторяет запрос без закрепления ключа — ступень стоит вдвое.
        assert_eq!(
            address_budget(Some(&option(Some(20), Some(true)))),
            Duration::from_secs(240) + LADDER_SLACK
        );
    }

    #[test]
    fn a_users_own_timeout_is_never_undercut() {
        // Числа здесь посчитаны руками, а не теми же константами, что и код: иначе
        // тест был бы тождественно истинным и уронённую константу не поймал бы.
        // Лестница — три ступени; каждая может быть повторена с запасными корнями
        // TLS; в защищённом канале — ещё раз без закрепления ключа прослойки.
        for (timeout, secure, honest_seconds) in [
            (1_u64, None, 6_u64),
            (5, None, 30),
            (20, None, 120),
            (60, None, 360),
            (600, None, 3600),
            (1, Some(true), 12),
            (20, Some(true), 240),
            (600, Some(true), 7200),
        ] {
            let budget = address_budget(Some(&option(Some(timeout), secure)));
            assert!(
                budget >= Duration::from_secs(honest_seconds),
                "бюджет {budget:?} короче честной лестницы {honest_seconds} с при timeout={timeout}"
            );
        }
    }

    #[test]
    fn an_absurd_timeout_does_not_panic_on_the_deadline() {
        // Именно здесь и была бы паника: `Instant + Duration::MAX`.
        for timeout in [u64::MAX, u64::MAX / 2, 1_000_000_000_000_000_000] {
            for secure in [None, Some(true)] {
                let budget = address_budget(Some(&option(Some(timeout), secure)));
                let deadline = tokio::time::Instant::now() + budget;
                assert!(deadline > tokio::time::Instant::now());
            }
        }
    }

    #[test]
    fn the_budget_never_hides_a_real_reason() {
        let real = anyhow::anyhow!("clod-sub-link-list: the panel returned a base64 link list");
        let budget = anyhow::anyhow!("clod-sub-budget: адрес подписки не ответил за отведённое время");

        assert!(
            keep_the_clearer_error(real, budget)
                .to_string()
                .contains("clod-sub-link-list")
        );
    }

    #[test]
    fn a_named_reason_is_not_lost_to_a_nameless_network_failure() {
        let named =
            anyhow::anyhow!("clod-sub-downgrade: the subscription address redirects to an insecure http address");
        let nameless = anyhow::anyhow!("failed to fetch remote profile");

        assert!(
            keep_the_clearer_error(named, nameless)
                .to_string()
                .contains("clod-sub-downgrade")
        );
    }

    #[test]
    fn a_real_reason_replaces_an_earlier_budget_failure() {
        let budget = anyhow::anyhow!("clod-sub-budget: адрес подписки не ответил за отведённое время");
        let real = anyhow::anyhow!("failed to fetch remote profile with status 403 Forbidden");

        assert!(keep_the_clearer_error(budget, real).to_string().contains("403"));
    }
}

#[cfg(test)]
mod update_claim_tests {
    use super::claim_update;

    #[test]
    fn a_profile_is_claimed_once_until_the_claim_is_dropped() {
        let uid = super::String::from("claim-test-uid");
        let first = claim_update(&uid);
        assert!(first.is_some(), "первый запуск обновления берёт профиль");
        assert!(
            claim_update(&uid).is_none(),
            "второй запуск того же профиля — кнопкой или расписанием — отклоняется"
        );
        assert!(
            claim_update(&super::String::from("claim-test-other")).is_some(),
            "другой профиль не задет"
        );
        drop(first);
        assert!(
            claim_update(&uid).is_some(),
            "после окончания обновления профиль снова свободен"
        );
    }
}
