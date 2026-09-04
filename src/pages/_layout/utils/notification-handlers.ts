import { Fragment, type KeyboardEvent, createElement } from 'react'

import { patchVergeConfig } from '@/services/cmds'
import { hideNotice, showNotice } from '@/services/notice-service'
import { revalidateQueries } from '@/services/query-client'
import getSystem from '@/utils/get-system'

const OS = getSystem()

type NavigateFunction = (path: string, options?: any) => void
type TranslateFunction = (
  key: string,
  params?: Record<string, unknown>,
) => string

const offerToTurnTheProxyOff = (
  messageKey: string,
  t: TranslateFunction,
): void => {
  let id = 0
  const disableProxy = () => {
    patchVergeConfig({ enable_system_proxy: false })
      .then(() => {
        hideNotice(id)
        return revalidateQueries([
          ['getVergeConfig'],
          ['getSystemProxy'],
          ['getAutotemProxy'],
        ])
      })
      .catch((err) => showNotice.error(err))
  }
  id = showNotice.error(
    createElement(
      Fragment,
      null,
      t(messageKey),
      ' ',
      createElement(
        'a',
        {
          role: 'button',
          tabIndex: 0,
          onClick: disableProxy,
          onKeyDown: (event: KeyboardEvent<HTMLAnchorElement>) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault()
              disableProxy()
            }
          },
          style: { cursor: 'pointer', textDecoration: 'underline' },
        },
        t('settings.sections.system.notifications.sysproxy.turnOffAction'),
      ),
    ),
    0,
  )
}

export const handleNoticeMessage = (
  status: string,
  msg: string,
  t: TranslateFunction,
  navigate: NavigateFunction,
) => {
  const handlers: Record<string, () => void> = {
    'import_sub_url::ok': () => {
      navigate('/profile')
      showNotice.success(
        'shared.feedback.notifications.importSubscriptionSuccess',
      )
    },
    'import_sub_url::error': () => {
      navigate('/profile')
      showNotice.error(msg)
    },
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
    'clod_core::updated': () =>
      showNotice.success('settings.modals.managedCore.updatedTo', msg),
    'clod_core::update_available': () =>
      showNotice.info('settings.modals.managedCore.updateNotice', msg),
    'reactivate_profiles::error': () => showNotice.error(msg),
    // clod:Э10-02 — файл настроек не прочитался; мы его не перезаписываем, но
    // человек должен знать, почему список выглядит пустым.
    'clod_config::load_failed': () =>
      showNotice.error(
        'shared.feedback.notifications.common.configLoadFailed',
        { files: msg },
        0,
      ),
    update_failed: () => showNotice.error(msg),
    'config_validate::boot_error': () =>
      showNotice.error('shared.feedback.validation.config.bootFailed', msg),
    'config_validate::core_change': () =>
      showNotice.error(
        'shared.feedback.validation.config.coreChangeFailed',
        msg,
      ),
    'config_validate::error': () =>
      showNotice.error('shared.feedback.validation.config.failed', msg),
    'config_validate::process_terminated': () =>
      // clod:Э10-12 — текст отказа нужен и здесь: на пути обновления подписки он
      // единственный говорит, на чём именно проверка оборвалась.
      showNotice.error(
        'shared.feedback.validation.config.processTerminated',
        msg,
      ),
    'config_validate::stdout_error': () =>
      showNotice.error('shared.feedback.validation.config.failed', msg),
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
    'config_validate::yaml_key_error': () =>
      showNotice.error('shared.feedback.validation.yaml.keyError', msg),
    'config_validate::yaml_error': () =>
      showNotice.error('shared.feedback.validation.yaml.generalError', msg),
    'config_validate::merge_syntax_error': () =>
      showNotice.error('shared.feedback.validation.merge.syntaxError', msg),
    'config_validate::merge_mapping_error': () =>
      showNotice.error('shared.feedback.validation.merge.mappingError', msg),
    'config_validate::merge_key_error': () =>
      showNotice.error('shared.feedback.validation.merge.keyError', msg),
    'config_validate::merge_error': () =>
      showNotice.error('shared.feedback.validation.merge.generalError', msg),
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
    'core::crashed': () =>
      showNotice.error(
        'settings.sections.system.notifications.core.crashed',
        msg,
      ),
    'core::restarted': () =>
      showNotice.info(
        'settings.sections.system.notifications.core.restarted',
        msg,
      ),
    'core::not_ready': () =>
      showNotice.error(
        'settings.sections.system.notifications.core.notReady',
        msg,
      ),
    'core::handoff_failed': () =>
      showNotice.error(
        'settings.sections.system.notifications.core.handoffFailed',
        msg,
      ),
    'core::port_busy': () => {
      let id = 0
      const openSettings = () => {
        hideNotice(id)
        void navigate('/settings')
      }
      id = showNotice.error(
        createElement(
          Fragment,
          null,
          t('settings.sections.system.notifications.core.portBusy', {
            port: msg,
          }),
          ' ',
          createElement(
            'a',
            {
              role: 'button',
              tabIndex: 0,
              onClick: openSettings,
              onKeyDown: (event: KeyboardEvent<HTMLAnchorElement>) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault()
                  openSettings()
                }
              },
              style: { cursor: 'pointer', textDecoration: 'underline' },
            },
            t('settings.sections.system.notifications.core.portBusyAction'),
          ),
        ),
        0,
      )
    },
    'sysproxy::core_gave_up': () =>
      offerToTurnTheProxyOff(
        'settings.sections.system.notifications.sysproxy.coreGaveUp',
        t,
      ),
    'sysproxy::core_not_running': () =>
      offerToTurnTheProxyOff(
        'settings.sections.system.notifications.sysproxy.coreNotRunning',
        t,
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
      showNotice.success(
        'settings.feedback.notifications.clash.changeSuccess',
        msg,
      ),
    'config_core::change_error': () =>
      showNotice.error(
        'settings.feedback.notifications.clash.changeFailed',
        msg,
      ),
  }

  const handler = handlers[status]
  if (handler) {
    handler()
  } else {
    console.warn(`Необработанный статус уведомления: ${status}`)
  }
}
