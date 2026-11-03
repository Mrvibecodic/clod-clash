import { StorageOutlined } from '@mui/icons-material'
import { Box, Button } from '@mui/material'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ProviderDialog } from '@/components/shared/provider-dialog'
import { TypeBox } from '@/components/shared/provider-type-box'
import { useAppRefreshers, useRulesData } from '@/providers/app-data-context'
import { updateCoreRuleProvider } from '@/services/cmds'
import { enumText } from '@/utils/plugin-enum'

export const ProviderButton = () => {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const { ruleProviders } = useRulesData()
  const { refreshRules, refreshRuleProviders } = useAppRefreshers()

  if (Object.keys(ruleProviders || {}).length === 0) return null

  return (
    <>
      <Button
        variant="outlined"
        size="small"
        startIcon={<StorageOutlined />}
        onClick={() => setOpen(true)}
      >
        {t('rules.page.provider.title')}
      </Button>

      <ProviderDialog
        open={open}
        onClose={() => setOpen(false)}
        title={t('rules.page.provider.title')}
        providers={ruleProviders}
        updateOne={updateCoreRuleProvider}
        refresh={async () => {
          await refreshRules()
          await refreshRuleProviders()
        }}
        noneKey="rules.feedback.notifications.provider.none"
        allUpdatedKey="rules.feedback.notifications.provider.allUpdated"
        badges={(provider) => (
          <TypeBox component="span">{provider.ruleCount}</TypeBox>
        )}
        details={(provider) => (
          <Box sx={{ display: 'flex' }}>
            <TypeBox component="span">{enumText(provider.vehicleType)}</TypeBox>
            <TypeBox component="span">{enumText(provider.behavior)}</TypeBox>
          </Box>
        )}
      />
    </>
  )
}
