use crate::{
    cmd,
    config::{
        Config, PrfItem, PrfOption,
        profiles::{UpdateMarks, profiles_draft_update_item_safe},
        sub_headers,
    },
    core::{
        CoreManager, handle,
        manager::{Applied, Delivered, Delivery},
        tray,
        validate::ValidationOutcome,
    },
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
    new_sub: Option<String>,
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
            option: item.fetch_option(),
            new_sub: item.new_sub.clone(),
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
    /// Файл на диске заменён, реестр обновлён; `delivered` — что стало с ядром,
    /// если профиль текущий (`None` — не текущий, ядру не отдавался);
    /// `freeze_pass_asked` — за время доставки уже позвали заход проверки 16–20
    /// (сама доставка — на новые узлы, или кто-то ещё), и он увидит реестр
    /// с новой записью.
    Accepted {
        delivered: Option<Delivered>,
        freeze_pass_asked: bool,
    },
    /// Ядро отвергло собранный из неё конфиг — на проверке (файл на диске прежний)
    /// или уже при доставке (файл заменён, ядро осталось на прежнем). Реестр (срок,
    /// трафик, замки панели, отметка загрузки) обновлён как при приёме, у профиля
    /// пометка «не применено».
    Rejected(ValidationOutcome),
    /// Файл принят и заменён, но доставить ядру не удалось (не поднялось, служба
    /// молчит): работает прежний конфиг, у профиля пометка «не применено».
    DeliveryFailed(anyhow::Error),
    /// До проверки не дошло — идёт выход: файл и реестр прежние, пометок нет,
    /// загрузку надо повторить скоро.
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
async fn accept_the_download(uid: &String, mut item: PrfItem, move_to: Option<Move>) -> Result<Acceptance> {
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

    // Реестр-кандидат выводится из принятого уже в своей очереди применения: за
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

    let staged = match CoreManager::global().stage_unless_unchanged(sources).await {
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

    let request_option = item.option.clone();
    if let Err(err) = promote_and_record(uid, item, &dir, &file, &candidate_path, disarming).await {
        let _ = tokio::fs::remove_file(&candidate_path).await;
        return Err(err);
    }

    let acceptance = if Config::profiles().await.data_arc().is_current_profile_index(uid) {
        deliver_the_accepted(uid, staged).await
    } else {
        drop(staged);
        Acceptance::Accepted {
            delivered: None,
            freeze_pass_asked: false,
        }
    };
    // Уже без признака применения: здесь запрос в сеть.
    follow_move(uid, move_to, request_option).await;
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
            if profiles_draft_update_item_safe(uid, &mut item, UpdateMarks::UNCHECKED).await? {
                handle::Handle::refresh_profiles();
            }
            Ok(Acceptance::Unchecked(outcome))
        }
        ValidationOutcome::Invalid { .. } => {
            logging!(
                warn,
                Type::Config,
                "[Обновление подписки] ядро отвергло новую подписку, рабочий файл не тронут: {}",
                outcome
            );
            profiles_draft_update_item_safe(uid, &mut item, UpdateMarks::REJECTED).await?;
            handle::Handle::refresh_profiles();
            Ok(Acceptance::Rejected(outcome))
        }
        ValidationOutcome::Valid | ValidationOutcome::Busy | ValidationOutcome::Skipped { .. } => {
            Ok(Acceptance::Unverified(outcome))
        }
    }
}

/// Прежний файл — в `<файл>.prev`, кандидат — на его место, метаданные — в реестр
/// одной записью вместе с пометками: загрузка удалась, прежнее «не применено»
/// относилось к прежнему содержимому.
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
    let failed_mark_changed = Config::profiles()
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
            let failed_mark_changed = profiles.update_item(&owner, &mut item, UpdateMarks::ACCEPTED).await?;
            Ok((profiles, failed_mark_changed))
        })
        .await?;
    if failed_mark_changed {
        handle::Handle::refresh_profiles();
    }
    Ok(())
}

/// Профиль текущий — та же проверенная сборка уходит ядру без второй проверки.
/// Если собранное совпало с работающим, ядро не трогается.
async fn deliver_the_accepted(uid: &String, staged: crate::core::manager::Staged<'_>) -> Acceptance {
    let asked = crate::module::freeze_check::passes_asked();
    match staged.deliver(Delivery::Reload).await {
        Ok(Ok(delivered)) => Acceptance::Accepted {
            delivered: Some(delivered),
            freeze_pass_asked: crate::module::freeze_check::passes_asked() != asked,
        },
        Ok(Err(outcome)) => {
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

/// Панель велела перевести подписку на запасной адрес (`clod-move-sub`): адрес
/// отдал годную подписку — он становится основным, иначе ждём следующего
/// обновления. Отказ по устройству — не повод: основной даст тот же отказ.
/// Основной адрес, пока шла проверка, сменил человек — перевод не применяется.
async fn follow_move(uid: &String, move_to: Option<Move>, request_option: Option<PrfOption>) {
    let Some(Move { from, to, served }) = move_to else {
        return;
    };

    let served = match served {
        Some(served) => Ok(served),
        None => PrfItem::from_url_with_ladder(&to, None, None, request_option.as_ref())
            .await
            .map(|fetched| fetched.item.device_refused != Some(true)),
    };
    let verdict = match served {
        Ok(true) => crate::config::profiles::profiles_move_url_safe(uid, from, to.clone()).await,
        Ok(false) => Err(anyhow::anyhow!("the panel refused this device")),
        Err(err) => Err(err),
    };
    match verdict {
        Ok(()) => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] [clod] provider moved the subscription to {}",
                mask_url(&to)
            );
            handle::Handle::notice_message("clod_sub::url_migrated", mask_url(&to));
        }
        Err(err) => logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] [clod] spare address {} is not taken as the main one yet: {}",
            mask_url(&to),
            mask_err(&err.to_string())
        ),
    }
}

/// Скачанная подписка и уведомление о том, каким путём она пришла: уведомление
/// уходит только после того, как подписку принял ядро.
struct Downloaded {
    item: PrfItem,
    notice: Option<&'static str>,
    /// Панель велела перевести подписку (`clod-move-sub`): с какого основного
    /// адреса и на какой запасной.
    move_to: Option<Move>,
}

/// Перевод подписки с основного адреса, с которого шло обновление, на запасной.
struct Move {
    from: String,
    to: String,
    /// Подписка только что пришла с этого самого запасного адреса: `true` — годная,
    /// `false` — отказ по устройству. `None` — адрес ещё не спрашивали.
    served: Option<bool>,
}

/// Перевод, если панель его велела: запасной адрес строится от основного адреса
/// этой подписки, с которого шло обновление, — не от адреса, откуда пришёл ответ.
fn move_of(url: &String, item: &PrfItem) -> Option<Move> {
    let domain = item.new_sub.as_deref().filter(|_| item.move_sub == Some(true))?;
    let to = sub_headers::spare_address(url, domain)?;
    Some(Move {
        from: url.clone(),
        to,
        served: None,
    })
}

/// Перевод, если подписка пришла с запасного адреса `spare`: велено перейти на
/// него же — второй раз его не спрашиваем.
fn move_after_the_spare(url: &String, spare: &String, item: &PrfItem) -> Option<Move> {
    move_of(url, item).map(|planned| Move {
        served: (planned.to == *spare).then_some(item.device_refused != Some(true)),
        ..planned
    })
}

/// Подписка скачана не напрямую, а через прокси (Clash или системный).
pub(crate) const UPDATED_VIA_PROXY: &str = "update_with_clash_proxy";

async fn perform_profile_update(
    uid: &String,
    url: &String,
    opt: Option<&PrfOption>,
    option: Option<&PrfOption>,
    new_sub: Option<String>,
) -> Result<Downloaded> {
    logging!(
        info,
        Type::Config,
        "[Обновление подписки] Начинаю загрузку нового содержимого подписки"
    );
    let merged_opt = PrfOption::merge(opt, option);
    let profiles = Config::profiles().await;
    let profiles_arc = profiles.latest_arc();
    let profile_name = profiles_arc
        .get_name_by_uid(uid)
        .unwrap_or_else(|| String::from("UnKnown Profile"));

    let mut last_err = match PrfItem::from_url_with_ladder(url, None, None, merged_opt.as_ref()).await {
        Ok(fetched) => {
            logging!(info, Type::Config, "[Обновление подписки] Подписка скачана");
            return Ok(Downloaded {
                move_to: move_of(url, &fetched.item),
                item: fetched.item,
                notice: fetched.detoured.then_some(UPDATED_VIA_PROXY),
            });
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] Основной адрес не ответил ни одним маршрутом: {}",
                mask_err(&err.to_string())
            );
            err
        }
    };

    if let Some(spare) = new_sub.and_then(|domain| sub_headers::spare_address(url, &domain)) {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] [clod] primary URL failed, trying the provider spare address {}",
            mask_url(&spare)
        );

        // У запасного адреса свой бюджет (он внутри лестницы): запасной существует
        // ровно для того случая, когда основной адрес молчит до последней секунды.
        match PrfItem::from_url_with_ladder(&spare, None, None, merged_opt.as_ref()).await {
            Ok(fetched) => {
                let mut item = fetched.item;
                item.from_fallback = Some(true);
                drop(last_err);
                return Ok(Downloaded {
                    move_to: move_after_the_spare(url, &spare, &item),
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

/// Текст отказа, пригодный для показа пользователю: адреса подписки, токены и
/// домашний каталог (в сообщении ядра приезжает полный путь — с именем
/// пользователя ОС) — вычищены, как в ответах команд.
fn public_failure_text(raw: &str) -> String {
    cmd::public_error_text(&mask_err(raw))
}

/// Сообщить о провале фонового обновления. `message` уже вычищен
/// ([`public_failure_text`]).
///
/// Только про текущий профиль: расписание догоняет пропущенные задания пачкой, и
/// при выключенной сети человек получил бы столько красных тостов, сколько у него
/// подписок. О фоновых провалах остальных говорит пометка на их карточках.
async fn announce_the_failure(uid: &String, status: &str, message: &str) {
    let is_current = Config::profiles().await.latest_arc().is_current_profile_index(uid);
    if !is_current {
        return;
    }

    handle::Handle::notice_message(status, message);
}

/// Добавить подписку: запись в реестр, реестр на диск, расписание. Что сказать
/// человеку о неудаче, решает вызывающий.
pub async fn add_profile(item: &mut PrfItem) -> Result<()> {
    crate::config::profiles_append_item_safe(item).await?;
    crate::config::profiles::profiles_save_file_safe().await?;
    logging_error!(Type::Timer, crate::core::Timer::global().refresh().await);
    Ok(())
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

/// Ядро приняло пересобранный конфиг: окну — перечитать, выбор узлов — по
/// тому, что стало с ядром.
pub(crate) fn settle_after_delivery(delivered: Delivered) {
    handle::Handle::refresh_clash();
    if let Err(err) = delivered.restore_selection() {
        logging!(warn, Type::Config, "Warning: restore selection failed: {err}");
    }
}

/// Прибраться после того, как ядро приняло конфиг обновлённой подписки.
fn settle_after_a_successful_update(uid: &String) {
    // Пометку «скачано, но не применено» снимает сам путь применения конфига
    // (`core/manager/config.rs`) — там она снимается на всех путях сразу, включая
    // переключение профиля и ручную пересборку.
    logging!(info, Type::Config, "[Обновление подписки] Обновление успешно");
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
    /// Расписание, когда о провале уже сказано: карточка подписки уже помечена
    /// провалом или «не применено». Провал повторяется с бэкоффом, и тост на
    /// каждый повтор шёл бы раз за разом; пометка лежит на диске, поэтому серия
    /// не начинается заново и после перезапуска. Удача пометку снимает.
    ScheduledRetry,
}

impl UpdateTrigger {
    const fn is_manual(self) -> bool {
        matches!(self, Self::Manual)
    }

    const fn announces_failure(self) -> bool {
        matches!(self, Self::Scheduled)
    }

    /// Расписание по помеченной карточке — см. [`Self::ScheduledRetry`].
    const fn once_marked(self, marked: bool) -> Self {
        match self {
            Self::Scheduled if marked => Self::ScheduledRetry,
            other => other,
        }
    }
}

/// Карточка подписки уже помечена провалом обновления или «не применено».
async fn card_is_marked(uid: &String) -> bool {
    Config::profiles()
        .await
        .latest_arc()
        .get_item(uid)
        .is_ok_and(|item| item.update_failed == Some(true) || item.not_applied == Some(true))
}

/// Кнопка на профиле, которому нечего скачивать (локальный или автообновление
/// запрещено): пересобрать конфиг ядра из того, что уже лежит на диске.
async fn reapply_the_current_profile(uid: &String, trigger: UpdateTrigger) -> Result<()> {
    match apply_current_profile().await {
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

/// Загрузка удалась — это записано вместе с самой подпиской; расписание и окно
/// лимита устройств об этом знают.
async fn note_the_download(uid: &String) {
    logging_error!(Type::Timer, crate::core::Timer::global().refresh().await);
    announce_device_refusal(uid).await;
}

/// Звать ли заход проверки 16–20 после обновления подписки: панель включила
/// или выключила проверку заголовком, а заход после записи этого в реестр
/// ещё никто не позвал.
const fn asks_a_freeze_pass(was_on: bool, is_on: bool, already_asked: bool) -> bool {
    was_on != is_on && !already_asked
}

/// Скачанная подписка принята или отвергнута — сказать об этом тем, кому положено.
async fn settle_the_download(uid: &String, downloaded: Downloaded, trigger: UpdateTrigger) -> Result<UpdateOutcome> {
    let Downloaded { item, notice, move_to } = downloaded;
    let (profile_name, freeze_was_on) = {
        let profiles = Config::profiles().await.data_arc();
        (
            profiles
                .get_name_by_uid(uid)
                .unwrap_or_else(|| String::from("UnKnown Profile")),
            profiles.get_item(uid).is_ok_and(|item| item.freeze_check == Some(true)),
        )
    };
    let freeze_is_on = item.freeze_check == Some(true);
    let acceptance = match Box::pin(accept_the_download(uid, item, move_to)).await {
        Ok(acceptance) => acceptance,
        Err(err) => {
            mark_the_update(uid, true).await;
            return Err(failed(uid, "update_failed", "Обновление не удалось", &err.to_string(), trigger).await);
        }
    };
    // Запись о подписке уже обновлена, принята она ядром или нет. Панель
    // включила или выключила проверку 16–20 заголовком — заход, если его ещё не
    // позвала доставка (новые узлы). Только если это подписка, на которой
    // работает ядро: другую он всё равно не проверит.
    let already_asked = matches!(
        acceptance,
        Acceptance::Accepted {
            freeze_pass_asked: true,
            ..
        }
    );
    if asks_a_freeze_pass(freeze_was_on, freeze_is_on, already_asked)
        && Config::runtime().await.data_arc().profile_uid.as_deref() == Some(uid.as_str())
    {
        crate::module::freeze_check::check_again("the panel switched the check");
    }

    let delivered = match acceptance {
        Acceptance::Accepted { delivered, .. } => delivered,
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
            let err = failed(uid, status, "Ядро отвергло подписку", &outcome.to_string(), trigger).await;
            return Err(RefusedByTheCore(err.to_string().into()).into());
        }
        Acceptance::Unchecked(outcome) => {
            // Скачано, но не проверено: файл прежний, обновления не случилось
            // (пометка уже в реестре); расписание и окно лимита устройств про
            // загрузку всё же узнают.
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
    if let Some(delivered) = delivered {
        settle_after_delivery(delivered);
        settle_after_a_successful_update(uid);
    }
    Ok(UpdateOutcome::Done)
}

/// Ядро отвергло скачанную подписку. Это слово ядра о содержимом (шаблон панели,
/// незнакомый ядру узел, цепочка merge/script человека), а не сбой: повтор через
/// минуты скачал бы то же и получил бы тот же отказ — расписание ждёт обычного
/// срока, а не повторяет с бэкоффом.
#[derive(Debug)]
pub struct RefusedByTheCore(pub(crate) String);

impl std::fmt::Display for RefusedByTheCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RefusedByTheCore {}

/// Чем закончился запуск обновления — для расписания.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// Сделано (или скачивать было нечего): дальше по расписанию.
    Done,
    /// До проверки не дошло (подписка уже обновляется или идёт выход): повторить
    /// скоро, а не через интервал, и не считать провалом загрузки.
    RetrySoon,
}

/// Подписки, которые обновляются прямо сейчас — один вход у кнопки и у расписания,
/// поэтому и признак «идёт» живёт здесь, а не в планировщике.
static UPDATES_IN_FLIGHT: std::sync::LazyLock<parking_lot::Mutex<InFlight>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(InFlight::default()));

/// Снимок для окна: какие подписки обновляются и номер этого состояния. Номер
/// растёт при каждой смене набора, и окно берёт только снимок новее своего —
/// событию и ответу команды порядок прихода не важен.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct UpdatesInFlight {
    pub revision: u64,
    pub uids: Vec<String>,
}

#[derive(Debug, Default)]
struct InFlight {
    revision: u64,
    uids: std::collections::BTreeSet<String>,
}

impl InFlight {
    fn snapshot(&self) -> UpdatesInFlight {
        UpdatesInFlight {
            revision: self.revision,
            uids: self.uids.iter().cloned().collect(),
        }
    }

    /// Взять подписку; `None` — она уже обновляется, набор не сменился.
    fn claim(&mut self, uid: &String) -> Option<UpdatesInFlight> {
        self.uids.insert(uid.clone()).then(|| self.changed())
    }

    fn release(&mut self, uid: &String) -> Option<UpdatesInFlight> {
        self.uids.remove(uid).then(|| self.changed())
    }

    fn changed(&mut self) -> UpdatesInFlight {
        self.revision += 1;
        self.snapshot()
    }
}

/// Одновременных загрузок подписок по расписанию — не больше стольких:
/// просроченные на старте идут очередью, а не залпом лестниц к панели.
const PARALLEL_DOWNLOADS: usize = 3;
static DOWNLOAD_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(PARALLEL_DOWNLOADS);

/// Заявка на обновление подписки. Взятие и снятие (любым путём, в том числе
/// отменой задачи) шлют окну снимок набора — признак «идёт» у всех кнопок
/// берётся из него, кнопкой обновляют или расписанием.
struct UpdateClaim(String);

impl Drop for UpdateClaim {
    fn drop(&mut self) {
        let released = UPDATES_IN_FLIGHT.lock().release(&self.0);
        if let Some(snapshot) = released {
            handle::Handle::notify_updates_in_flight(&snapshot);
        }
    }
}

/// Тот же снимок, что уходит окну событием: по нему окно сверяется при
/// появлении и показе, когда события могли пройти мимо него.
pub fn updates_in_flight() -> UpdatesInFlight {
    UPDATES_IN_FLIGHT.lock().snapshot()
}

fn claim_update(uid: &String) -> Option<UpdateClaim> {
    let snapshot = UPDATES_IN_FLIGHT.lock().claim(uid)?;
    handle::Handle::notify_updates_in_flight(&snapshot);
    Some(UpdateClaim(uid.clone()))
}

pub async fn update_profile(
    uid: &String,
    option: Option<&PrfOption>,
    ignore_auto_update: bool,
    trigger: UpdateTrigger,
) -> Result<UpdateOutcome> {
    let Some(claim) = claim_update(uid) else {
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
    let outcome = Box::pin(update_claimed(uid, option, ignore_auto_update, trigger)).await;
    drop(claim);
    outcome
}

async fn update_claimed(
    uid: &String,
    option: Option<&PrfOption>,
    ignore_auto_update: bool,
    trigger: UpdateTrigger,
) -> Result<UpdateOutcome> {
    let trigger = trigger.once_marked(card_is_marked(uid).await);
    logging!(
        info,
        Type::Config,
        "[Обновление подписки] Начинаю обновление подписки {} ({trigger:?})",
        uid
    );
    let url_opt = match should_update_profile(uid, ignore_auto_update).await {
        Ok(target) => target,
        Err(err) => {
            mark_the_update(uid, true).await;
            // Ручной вызов покажет ошибку сам — она вернётся ответом команды.
            if trigger.announces_failure() {
                announce_the_failure(uid, "update_failed", &public_failure_text(&err.to_string())).await;
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
            target.new_sub,
        ))
        .await
    };
    let downloaded = match downloaded {
        Ok(downloaded) => downloaded,
        Err(err) => {
            // Расписание сняло просроченные замки панели в начале тика.
            if trigger.is_manual() {
                release_stale_panel_locks().await;
            }
            mark_the_update(uid, true).await;
            // Загрузка провалилась. Ручной вызов покажет ошибку сам — она
            // уедет наверх и вернётся в интерфейс ответом команды; а вот
            // автообновление до этой правки не сообщало о провале никак:
            // расписание только писало в журнал.
            if trigger.announces_failure() {
                announce_the_failure(uid, "update_failed", &public_failure_text(&err.to_string())).await;
            }
            return Err(err);
        }
    };

    let outcome = Box::pin(settle_the_download(uid, downloaded, trigger)).await;
    // clod:report — отчёт прослойке уходит только после планового обновления.
    if outcome.is_ok() && !trigger.is_manual() {
        let uid = uid.to_string();
        crate::process::AsyncHandler::spawn(move || crate::module::client_report::after_scheduled_update(uid));
    }
    outcome
}

/// Пересобрать и отдать ядру всегда — даже если сборка не изменилась (горячая
/// клавиша «Переприменить подписки», удаление и откат удаления, правка файла).
pub async fn enhance_profiles() -> Result<ValidationOutcome> {
    Ok(settle_the_reapply(CoreManager::global().update_config_forced().await?))
}

/// Пересобрать текущую подписку с диска и отдать ядру, только если сборка
/// изменилась (правка карточки, кнопка «Обновить» у локальной подписки).
pub async fn apply_current_profile() -> Result<ValidationOutcome> {
    Ok(settle_the_reapply(
        CoreManager::global().update_config_unless_unchanged().await?,
    ))
}

fn settle_the_reapply(applied: Applied) -> ValidationOutcome {
    match applied {
        Ok(delivered) => {
            settle_after_delivery(delivered);
            ValidationOutcome::Valid
        }
        Err(outcome) => outcome,
    }
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
    use super::{UpdateTrigger, failure_notice_status, public_failure_text};
    use crate::core::validate::{ValidationErrorKind, ValidationOutcome};

    #[test]
    fn a_scheduled_failure_is_announced_once_per_marked_card() {
        // Тост — первому провалу серии; пока карточка помечена, повторы
        // расписания (и после перезапуска) идут молча. Кнопку это не касается.
        let cases = [
            (UpdateTrigger::Scheduled, false, UpdateTrigger::Scheduled),
            (UpdateTrigger::Scheduled, true, UpdateTrigger::ScheduledRetry),
            (UpdateTrigger::Manual, true, UpdateTrigger::Manual),
            (UpdateTrigger::Manual, false, UpdateTrigger::Manual),
        ];
        for (trigger, marked, expected) in cases {
            assert_eq!(trigger.once_marked(marked), expected, "{trigger:?}, marked {marked}");
            assert_eq!(
                trigger.once_marked(marked).announces_failure(),
                expected == UpdateTrigger::Scheduled
            );
        }
    }

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
    fn a_failure_text_is_cleaned_once() {
        // Адрес подписки с токеном и секрет вычищены, слова отказа остались.
        let raw =
            "error sending request for url (https://panel.example.com/sub/s3cr3tt0ken?flag=clash) secret: hunter2";
        let shown = public_failure_text(raw);
        assert_eq!(
            shown,
            "error sending request for url (https://panel.example.com/*** secret: ***"
        );
        // Окно получает уже вычищенный текст: вторая чистка его не меняет.
        assert_eq!(public_failure_text(&shown), shown);

        // Чистит тот, кто готовит текст, а не объявление.
        let source = include_str!("profile.rs");
        let announce = crate::utils::source_scan::fn_body(source, "async fn announce_the_failure(").unwrap_or_default();
        assert!(
            !announce.is_empty(),
            "тело announce_the_failure не найдено — тест ослеп"
        );
        assert!(!announce.contains("public_failure_text"), "{announce}");

        let update: std::string::String = crate::utils::source_scan::fn_body(source, "async fn update_claimed(")
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        assert!(
            update.contains("iftrigger.is_manual(){release_stale_panel_locks().await;}"),
            "расписание сняло замки в начале тика: {update}"
        );
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
mod update_error_tests {
    use super::keep_the_clearer_error;

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
    use super::{InFlight, String, UpdatesInFlight};

    fn uid(name: &str) -> String {
        String::from(name)
    }

    #[test]
    fn a_profile_is_claimed_once_until_it_is_released() {
        let mut in_flight = InFlight::default();
        assert!(in_flight.claim(&uid("a")).is_some(), "первый запуск берёт подписку");
        assert!(
            in_flight.claim(&uid("a")).is_none(),
            "второй запуск той же подписки — кнопкой или расписанием — отклоняется"
        );
        assert!(in_flight.claim(&uid("b")).is_some(), "другая подписка не задета");
        assert!(in_flight.release(&uid("a")).is_some());
        assert!(
            in_flight.claim(&uid("a")).is_some(),
            "после окончания подписка снова свободна"
        );
    }

    #[test]
    fn every_change_of_the_set_raises_the_revision_and_the_snapshot_is_the_set() {
        let mut in_flight = InFlight::default();
        assert_eq!(in_flight.snapshot(), UpdatesInFlight::default());

        let steps = [
            (in_flight.claim(&uid("b")), 1, vec!["b"]),
            (in_flight.claim(&uid("a")), 2, vec!["a", "b"]),
            (in_flight.release(&uid("b")), 3, vec!["a"]),
            (in_flight.release(&uid("a")), 4, vec![]),
        ];
        for (snapshot, revision, uids) in steps {
            let expected = UpdatesInFlight {
                revision,
                uids: uids.into_iter().map(uid).collect(),
            };
            assert_eq!(snapshot, Some(expected));
        }
        assert_eq!(
            in_flight.snapshot().revision,
            4,
            "снимок по запросу — тот же, что ушёл последним"
        );
    }

    #[test]
    fn a_refused_claim_or_an_empty_release_changes_nothing() {
        let mut in_flight = InFlight::default();
        let taken = in_flight.claim(&uid("a"));
        assert!(in_flight.claim(&uid("a")).is_none());
        assert!(in_flight.release(&uid("b")).is_none());
        assert_eq!(
            Some(in_flight.snapshot()),
            taken,
            "номер не сдвинулся — окну нечего слать"
        );
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod move_tests {
    use super::{move_after_the_spare, move_of};
    use crate::config::PrfItem;
    use smartstring::alias::String;

    fn answer(new_sub: Option<&str>, move_sub: bool) -> PrfItem {
        PrfItem {
            new_sub: new_sub.map(Into::into),
            move_sub: move_sub.then_some(true),
            ..PrfItem::default()
        }
    }

    #[test]
    fn the_move_is_built_from_the_main_address_of_this_subscription() {
        let main = String::from("https://main.example/sub/token?x=1");
        let planned = move_of(&main, &answer(Some("spare.example"), true)).expect("move is ordered");
        assert_eq!(planned.from, main);
        assert_eq!(planned.to, "https://spare.example/sub/token?x=1");
    }

    #[test]
    fn a_spare_that_just_served_the_subscription_is_not_asked_again() {
        let main = String::from("https://main.example/sub/token");
        let spare = String::from("https://spare.example/sub/token");

        let served = move_after_the_spare(&main, &spare, &answer(Some("spare.example"), true)).expect("move");
        assert_eq!(served.served, Some(true));

        let mut refused = answer(Some("spare.example"), true);
        refused.device_refused = Some(true);
        let refused = move_after_the_spare(&main, &spare, &refused).expect("move");
        assert_eq!(refused.served, Some(false), "отказ по устройству тоже уже известен");

        let elsewhere = move_after_the_spare(&main, &spare, &answer(Some("third.example"), true)).expect("move");
        assert_eq!(elsewhere.served, None, "другой адрес ещё не спрашивали");
        assert!(move_of(&main, &answer(Some("spare.example"), true)).is_some_and(|planned| planned.served.is_none()));
    }

    #[test]
    fn no_move_without_the_flag_the_domain_or_on_the_same_host() {
        let main = String::from("https://main.example/sub");
        assert!(move_of(&main, &answer(Some("spare.example"), false)).is_none());
        assert!(move_of(&main, &answer(None, true)).is_none());
        assert!(move_of(&main, &answer(Some("main.example"), true)).is_none());
    }
}

#[cfg(test)]
mod one_write_tests {
    use crate::utils::source_scan::fn_body;

    fn squeezed(source: &str, signature: &str) -> std::string::String {
        let body: std::string::String = fn_body(source, signature)
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        assert!(!body.is_empty(), "тело {signature} не найдено — тест ослеп");
        body
    }

    #[test]
    fn the_outcome_marks_ride_in_the_same_write_as_the_subscription() {
        let source = include_str!("profile.rs");
        let promote = squeezed(source, "async fn promote_and_record(");
        assert!(promote.contains("UpdateMarks::ACCEPTED"), "{promote}");
        assert!(!promote.contains("profiles_mark_not_applied"), "{promote}");

        let refused = squeezed(source, "async fn refused_before_the_disk(");
        assert!(refused.contains("UpdateMarks::REJECTED") && refused.contains("UpdateMarks::UNCHECKED"));
        assert!(!refused.contains("mark_not_applied("), "{refused}");

        let noted = squeezed(source, "async fn note_the_download(");
        assert!(!noted.contains("mark_the_update("), "{noted}");
        let settle = squeezed(source, "async fn settle_the_download(");
        assert_eq!(
            settle.matches("mark_the_update(").count(),
            2,
            "только провал приёма и «до проверки не дошло»: {settle}"
        );
    }

    #[test]
    fn the_window_hears_of_every_change_of_the_claims() {
        let source = include_str!("profile.rs");
        let claim = squeezed(source, "fn claim_update(");
        assert!(
            claim.contains("lock().claim(uid)?;handle::Handle::notify_updates_in_flight(&snapshot)"),
            "{claim}"
        );
        let release = squeezed(source, "impl Drop for UpdateClaim");
        assert!(release.contains("lock().release(&self.0)"), "{release}");
        assert!(
            release.contains("handle::Handle::notify_updates_in_flight(&snapshot)"),
            "{release}"
        );

        let timer = crate::utils::source_scan::production_code(include_str!("../core/timer.rs"));
        assert!(
            !timer.contains("notify_updates_in_flight"),
            "расписание само о заявках не говорит"
        );
    }

    #[test]
    fn deleting_a_subscription_writes_the_registry_once() {
        let commands = include_str!("../cmd/profile.rs");
        let delete = squeezed(commands, "pub async fn delete_profile(");
        assert!(!delete.contains("profiles_save_file_safe"), "{delete}");
        // Не текущую удаляют, только если её не сделали текущей, пока шли сюда:
        // иначе ядро осталось бы на сборке удалённой.
        assert!(!delete.contains("profiles_delete_item_safe"), "{delete}");
        assert!(delete.contains("delete_unless_current(&index)"), "{delete}");
        let deliver = squeezed(commands, "async fn deliver_without(");
        assert!(deliver.contains("deliver_committing("), "{deliver}");
        assert!(!commands.contains("restore_profiles_after_failed_delete"));
    }
}

#[cfg(test)]
mod freeze_pass_tests {
    use super::asks_a_freeze_pass;

    #[test]
    fn an_update_asks_a_check_only_when_the_panel_switched_it_and_nobody_asked_yet() {
        assert!(asks_a_freeze_pass(false, true, false), "панель включила проверку");
        assert!(
            asks_a_freeze_pass(true, false, false),
            "панель выключила — пометки гаснут"
        );
        assert!(!asks_a_freeze_pass(true, true, false), "флаг тот же — повода нет");
        assert!(!asks_a_freeze_pass(false, false, false));
        assert!(
            !asks_a_freeze_pass(false, true, true),
            "доставка уже позвала заход, он увидит новый флаг"
        );

        let source = include_str!("profile.rs");
        let settle = crate::utils::source_scan::fn_body(source, "async fn settle_the_download(").unwrap_or_default();
        let gate = settle.find("asks_a_freeze_pass(").unwrap_or(usize::MAX);
        let kick = settle.find("check_again(").unwrap_or(0);
        assert!(gate < kick, "заход — только по смене флага: {settle}");
        let deliver = crate::utils::source_scan::fn_body(source, "async fn deliver_the_accepted(").unwrap_or_default();
        assert!(deliver.contains("passes_asked()"), "{deliver}");
    }
}
