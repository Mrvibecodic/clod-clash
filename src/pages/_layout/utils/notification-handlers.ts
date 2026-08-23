import { showNotice } from '@/services/notice-service'
import getSystem from '@/utils/get-system'

const OS = getSystem()

type NavigateFunction = (path: string, options?: any) => void
type TranslateFunction = (key: string) => string

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
    update_failed_even_with_clash: () =>
      showNotice.error(
        'settings.feedback.notifications.updater.withClashProxyFailed',
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
      showNotice.error('shared.feedback.validation.config.processTerminated'),
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
        msg,
      ),
    'tun::start_failed': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.autoDisabled',
        msg,
      ),
    'tun::adapter_busy': () =>
      showNotice.error(
        'settings.sections.system.notifications.tunMode.adapterBusy',
        msg,
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
    'core::crashed': () =>
      showNotice.error(
        'settings.sections.system.notifications.core.crashed',
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
