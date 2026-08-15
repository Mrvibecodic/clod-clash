import { Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import { Switch } from '@/components/base'
import { TOOL_SHORTCUTS, useToolShortcuts } from '@/hooks/use-tool-shortcuts'
import { showNotice } from '@/services/notice-service'

import { SettingItem, SettingList } from './mods/setting-comp'

/**
 * clod:design-v2 — the technical sections that used to be tiles on the
 * advanced home screen. They are tools, not everyday actions, so they live
 * here: the home screen keeps room for the account button to come.
 *
 * clod:tool-shortcuts — и всё же дорога назад нужна: тумблер в строке выносит
 * инструмент ярлыком на главную расширенного режима. Настройка стоит именно
 * здесь, а не отдельным экраном «ярлыки»: решение принимают в тот момент,
 * когда видят перед собой список инструментов.
 */
const SettingTools = () => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { isEnabled, setEnabled } = useToolShortcuts()

  const toggle = useLockFn(async (key: string, next: boolean) => {
    try {
      await setEnabled(key, next)
    } catch (error) {
      showNotice.error(error)
    }
  })

  return (
    <SettingList title={t('settings.components.tools.title')}>
      {TOOL_SHORTCUTS.map((tool) => (
        <SettingItem
          key={tool.key}
          label={t(tool.label)}
          secondary={t(tool.hint)}
          action={
            <>
              <Typography variant="caption" color="text.secondary" noWrap>
                {t('settings.components.tools.shortcut')}
              </Typography>
              <Switch
                checked={isEnabled(tool.key)}
                slotProps={{
                  input: {
                    'aria-label': t('settings.components.tools.shortcutAria', {
                      name: t(tool.label),
                    }),
                  },
                }}
                onChange={(_event, next) => void toggle(tool.key, next)}
              />
            </>
          }
          onClick={() => void navigate(tool.path)}
        />
      ))}
    </SettingList>
  )
}

export default SettingTools
