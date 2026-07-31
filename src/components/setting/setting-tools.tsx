import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import { SettingItem, SettingList } from './mods/setting-comp'

/**
 * clod:design-v2 — the technical sections that used to be tiles on the
 * advanced home screen. They are tools, not everyday actions, so they live
 * here: the home screen keeps room for the account button to come.
 */
const SettingTools = () => {
  const { t } = useTranslation()
  const navigate = useNavigate()

  return (
    <SettingList title={t('settings.components.tools.title')}>
      <SettingItem
        label={t('layout.components.navigation.tabs.proxies')}
        secondary={t('home.pages.advanced.tiles.proxiesHint')}
        onClick={() => void navigate('/proxies')}
      />
      <SettingItem
        label={t('layout.components.navigation.tabs.rules')}
        secondary={t('home.pages.advanced.tiles.rulesHint')}
        onClick={() => void navigate('/rules')}
      />
      <SettingItem
        label={t('layout.components.navigation.tabs.connections')}
        secondary={t('home.pages.advanced.tiles.connectionsHint')}
        onClick={() => void navigate('/connections')}
      />
      <SettingItem
        label={t('layout.components.navigation.tabs.logs')}
        secondary={t('home.pages.advanced.tiles.logsHint')}
        onClick={() => void navigate('/logs')}
      />
    </SettingList>
  )
}

export default SettingTools
