import { Fragment, createElement } from 'react'

import {
  getProfiles,
  patchClashConfig,
  patchVergeConfig,
} from '@/services/cmds'
import { collapseBy, hideNotice, showNotice } from '@/services/notice-service'
import { getCacheData, revalidateQueries } from '@/services/query-client'
import getSystem from '@/utils/get-system'
import { runNoticeAction } from '@/utils/notice-action'
import { pressable } from '@/utils/pressable'
import { resolveSimpleMode } from '@/utils/simple-mode'

import { isNoticeStatus, type NoticeStatus } from './notice-statuses'

const OS = getSystem()

type NavigateFunction = (path: string, options?: any) => void
type TranslateFunction = (
  key: string,
  params?: Record<string, unknown>,
) => string

/**
 * Уведомление с действием: текст и ссылка-кнопка (мышь и клавиатура).
 * Действие удалось — уведомление закрывается, нет — причина отдельной ошибкой.
 */
const offerAnAction = (
  kind: 'error' | 'warning',
  text: string,
  label: string,
  act: () => unknown,
  collapseKey: string,
): number => {
  let id = 0
  const run = () =>
    void runNoticeAction(
      act,
      () => hideNotice(id),
      (err) => showNotice.error(err),
    )
  id = showNotice[kind](
    createElement(
      Fragment,
      null,
      text,
      ' ',
      createElement(
        'a',
        {
          ...pressable(run),
          onClick: run,
          style: { cursor: 'pointer', textDecoration: 'underline' },
        },
        label,
      ),
    ),
    collapseBy(collapseKey),
    0,
  )
  return id
}

const offerToTurnTheProxyOff = (
  messageKey: string,
  t: TranslateFunction,
  collapseKey: string,
): number =>
  offerAnAction(
    'error',
    t(messageKey),
    t('settings.sections.system.notifications.sysproxy.turnOffAction'),
    () =>
      patchVergeConfig({ enable_system_proxy: false }).then(() =>
        revalidateQueries([
          ['getVergeConfig'],
          ['getSystemProxy'],
          ['getAutotemProxy'],
        ]),
      ),
    collapseKey,
  )

export const handleNoticeMessage = (
  status: string,
  msg: string,
  t: TranslateFunction,
  navigate: NavigateFunction,
): number | undefined => {
  // Порт держит кто-то другой — ядро на нём не слушает. Смена порта лечит
  // любой случай, поэтому ссылка одна на все тексты.
  const offerToChangeThePort = (text: string, collapseKey: string): number =>
    offerAnAction(
      'error',
      text,
      t('settings.sections.system.notifications.core.portBusyAction'),
      () => navigate('/settings'),
      collapseKey,
    )

  // Подписка по ссылке провайдера — на Главную, к кнопке «Подключить»;
  // список подписок нужен только в расширенном режиме, когда их несколько.
  const openTheImportedSubscription = () => {
    const userChoice = getCacheData<IVergeConfig>([
      'getVergeConfig',
    ])?.simple_mode
    void getProfiles()
      .then((profiles) => {
        const items = profiles.items ?? []
        const current = items.find((item) => item.uid === profiles.current)
        const advanced = !resolveSimpleMode(userChoice, current?.simple_mode)
        const many = items.filter((item) => item.type === 'remote').length > 1
        navigate(advanced && many ? '/profile' : '/')
      })
      .catch(() => navigate('/'))
  }

  const handlers: Record<NoticeStatus, () => number> = {
    'import_sub_url::ok': () => {
      openTheImportedSubscription()
      return showNotice.success(
        'shared.feedback.notifications.importSubscriptionSuccess',
      )
    },
    'import_sub_url::exists': () => {
      openTheImportedSubscription()
      return showNotice.info(
        'shared.feedback.notifications.importSubscriptionExists',
      )
    },
    'import_sub_url::error': () => showNotice.error(msg),
    'set_config::ok': () =>
      showNotice.success(
        'settings.feedback.notifications.clash.restartSuccess',
      ),
    'set_config::error': () => showNotice.error(msg),
    update_with_clash_proxy: () =>
      showNotice.success(
        'settings.feedback.notifications.updater.withClashProxySuccess',
        msg,
      ),
    'clod_sub::url_migrated': () =>
      showNotice.success(
        'profiles.page.feedback.notifications.urlMigrated',
        msg,
      ),
    'clod_sub::fallback_used': () =>
      showNotice.info('profiles.page.feedback.notifications.fallbackUsed', msg),
    'reactivate_profiles::error': () => showNotice.error(msg),
    // clod:Э10-02 — файл настроек не прочитался; мы его не перезаписываем, но
    // человек должен знать, почему список выглядит пустым.
    'clod_config::load_failed': () =>
      showNotice.error(
        'shared.feedback.notifications.common.configLoadFailed',
        { files: msg },
        0,
      ),
    // clod:tun-owned-keys — merge/script записали ключ, которым управляют
    // настройки приложения; запись отброшена, человеку говорят какая.
    'clod_config::keys_discarded': () =>
      showNotice.warning(
        'shared.feedback.notifications.common.keysDiscarded',
        { keys: msg },
        0,
      ),
    'clod_config::lan_list_unreadable': () =>
      offerAnAction(
        'warning',
        t('shared.feedback.notifications.common.lanListUnreadable', {
          key: msg,
        }),
        t('shared.feedback.notifications.common.lanSharingOffAction'),
        () =>
          patchClashConfig({ 'allow-lan': false }).then(() =>
            revalidateQueries([['getRuntimeConfig'], ['getCoreLadder']]),
          ),
        `clod_config::lan_list_unreadable|${msg}`,
      ),
    update_failed: () => showNotice.error(msg),
    'update::breaking_changes': () =>
      showNotice.info(
        'settings.feedback.notifications.updater.breakingChanges',
        { version: msg },
        0,
      ),
    'update::install_failed': () => showNotice.error(msg),
    'config_validate::boot_error': () =>
      showNotice.error('shared.feedback.validation.config.bootFailed', msg),
    'config_validate::error': () =>
      showNotice.error('shared.feedback.validation.config.failed', msg),
    'config_validate::process_terminated': () =>
      // clod:Э10-12 — текст отказа нужен и здесь: на пути обновления подписки он
      // единственный говорит, на чём именно проверка оборвалась.
      showNotice.error(
        'shared.feedback.validation.config.processTerminated',
        msg,
      ),
    'config_validate::script_error': () =>
      showNotice.error('shared.feedback.validation.script.fileError', msg),
    'config_validate::script_syntax_error': () =>
      showNotice.error('shared.feedback.validation.script.syntaxError', msg),
    'config_validate::script_missing_main': () =>
      showNotice.error('shared.feedback.validation.script.missingMain', msg),
    'config_validate::file_not_found': () =>
      showNotice.error('shared.feedback.validation.script.fileNotFound', msg),
    'config_validate::yaml_syntax_error': () =>
      showNotice.error('shared.feedback.validation.yaml.syntaxError', msg),
    'config_validate::yaml_read_error': () =>
      showNotice.error('shared.feedback.validation.yaml.readError', msg),
    'config_validate::yaml_mapping_error': () =>
      showNotice.error('shared.feedback.validation.yaml.mappingError', msg),
    'config_validate::merge_syntax_error': () =>
      showNotice.error('shared.feedback.validation.merge.syntaxError', msg),
    'config_validate::merge_mapping_error': () =>
      showNotice.error('shared.feedback.validation.merge.mappingError', msg),
    'tun::setup_started': () =>
      showNotice.info(
        'settings.sections.system.notifications.tunMode.setupStarted',
      ),
    'tun::setup_done': () =>
      showNotice.success(
        'settings.sections.system.notifications.tunMode.setupDone',
      ),
    'tun::setup_failed': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.setupFailed',
      ),
    'tun::rights_declined': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.rightsDeclined',
      ),
    'tun::service_silent': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.serviceSilent',
      ),
    'tun::start_failed': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.autoDisabled',
      ),
    'tun::no_rights': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.noRights',
      ),
    'tun::adapter_busy': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.adapterBusy',
      ),
    'tun::no_traffic': () =>
      showNotice.error(
        OS === 'windows' && ['system', 'mixed'].includes(msg.toLowerCase())
          ? 'settings.sections.system.notifications.tunMode.noTrafficWindows'
          : 'settings.sections.system.notifications.tunMode.noTraffic',
        msg,
      ),
    'service::needs_repair': () =>
      showNotice.error(
        'settings.sections.system.notifications.service.needsRepair',
      ),
    'service::bundle_rejected': () =>
      showNotice.error(
        'settings.sections.system.notifications.service.bundleRejected',
        msg,
      ),
    'core::crashed': () => {
      void revalidateQueries([['getSystemState']])
      return showNotice.error(
        'settings.sections.system.notifications.core.crashed',
        msg,
      )
    },
    'core::restarted': () => {
      void revalidateQueries([['getSystemState']])
      return showNotice.info(
        'settings.sections.system.notifications.core.restarted',
        msg,
      )
    },
    'core::not_ready': () => {
      void revalidateQueries([['getSystemState']])
      return showNotice.error(
        'settings.sections.system.notifications.core.notReady',
        msg,
      )
    },
    'core::handoff_failed': () => {
      void revalidateQueries([['getSystemState']])
      return showNotice.error(
        'settings.sections.system.notifications.core.handoffFailed',
        msg,
      )
    },
    'app_quit::core_still_running': () => {
      void revalidateQueries([['getSystemState']])
      return showNotice.error(
        'settings.sections.system.notifications.core.exitCancelled',
        { reason: msg },
        0,
      )
    },
    'app_quit::in_progress': () =>
      showNotice.info(
        'settings.sections.system.notifications.core.exitInProgress',
      ),
    'core::port_busy': () =>
      offerToChangeThePort(
        `${t('settings.sections.system.notifications.core.portBusy', {
          port: msg,
        })} ${t('settings.sections.system.notifications.core.portBusyHint')}`,
        `core::port_busy|${msg}`,
      ),
    'core::port_held_by_our_copy': () =>
      offerToChangeThePort(
        t('settings.sections.system.notifications.core.portHeldByOurCopy', {
          port: msg,
        }),
        `core::port_held_by_our_copy|${msg}`,
      ),
    'sysproxy::core_gave_up': () =>
      offerToTurnTheProxyOff(
        'settings.sections.system.notifications.sysproxy.coreGaveUp',
        t,
        `sysproxy::core_gave_up|${msg}`,
      ),
    'sysproxy::core_not_running': () =>
      offerToTurnTheProxyOff(
        'settings.sections.system.notifications.sysproxy.coreNotRunning',
        t,
        `sysproxy::core_not_running|${msg}`,
      ),
    'sysproxy::guard_stood_down': () =>
      showNotice.error(
        'settings.sections.system.notifications.sysproxy.guardStoodDown',
      ),
    'sysproxy::write_failed': () =>
      showNotice.error(
        'settings.sections.system.notifications.sysproxy.writeFailed',
        msg,
      ),
    'core::binary_changed': () =>
      showNotice.error(
        'settings.sections.system.notifications.core.binaryChanged',
        msg,
      ),
    'config_core::change_success': () =>
      showNotice.success('settings.feedback.notifications.clash.changeSuccess'),
    'config_core::change_error': () =>
      showNotice.error(
        'settings.feedback.notifications.clash.changeFailed',
        msg,
      ),
  }

  if (!isNoticeStatus(status)) {
    console.warn(`Необработанный статус уведомления: ${status}`)
    return undefined
  }
  return handlers[status]()
}
