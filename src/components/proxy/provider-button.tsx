import { StorageOutlined } from '@mui/icons-material'
import { Box, Button, LinearProgress } from '@mui/material'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ProviderDialog } from '@/components/shared/provider-dialog'
import { TypeBox } from '@/components/shared/provider-type-box'
import { NARROW } from '@/pages/_theme'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import { updateCoreProxyProvider } from '@/services/cmds'
import { profileDate } from '@/utils/date-text'
import parseTraffic from '@/utils/parse-traffic'
import { enumText } from '@/utils/plugin-enum'

type ProxyProvider = NonNullable<
  ReturnType<typeof useProxiesData>['proxyProviders']
>[string]

/** Сведения подписки провайдера: трафик, срок и полоса расхода. */
const SubscriptionInfo = ({ provider }: { provider: ProxyProvider }) => {
  const { t } = useTranslation()
  const sub = provider.subscriptionInfo
  if (!sub) return null
  const upload = sub.Upload || 0
  const download = sub.Download || 0
  const total = sub.Total || 0
  const expire = sub.Expire || 0
  const progress =
    total > 0
      ? Math.min(Math.round(((download + upload) * 100) / total) + 1, 100)
      : 0

  return (
    <>
      <Box
        sx={{
          mb: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}
      >
        <span title={t('shared.labels.usedTotal') as string}>
          {parseTraffic(upload + download)} / {parseTraffic(total)}
        </span>
        <span title={t('shared.labels.expireTime') as string}>
          {profileDate(expire)}
        </span>
      </Box>
      <LinearProgress
        variant="determinate"
        value={progress}
        sx={{ height: 6, borderRadius: 3, opacity: total > 0 ? 1 : 0 }}
      />
    </>
  )
}

export const ProviderButton = () => {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const { proxyProviders } = useProxiesData()
  const { refreshProxy } = useAppRefreshers()

  if (Object.keys(proxyProviders || {}).length === 0) return null

  return (
    <>
      <Button
        variant="outlined"
        size="small"
        startIcon={<StorageOutlined />}
        onClick={() => setOpen(true)}
        title={t('proxies.page.provider.title')}
        sx={{
          mr: 1,
          whiteSpace: 'nowrap',
          [NARROW]: {
            minWidth: 0,
            '& .provider-label': { display: 'none' },
            '& .MuiButton-startIcon': { m: 0 },
          },
        }}
      >
        <span className="provider-label">
          {t('proxies.page.provider.title')}
        </span>
      </Button>

      <ProviderDialog
        open={open}
        onClose={() => setOpen(false)}
        title={t('proxies.page.provider.title')}
        providers={proxyProviders}
        updateOne={updateCoreProxyProvider}
        // Провайдеры приходят вместе с группами
        refresh={refreshProxy}
        noneKey="proxies.feedback.notifications.provider.none"
        allUpdatedKey="proxies.feedback.notifications.provider.allUpdated"
        badges={(provider) => (
          <>
            <TypeBox component="span">{provider.proxies.length}</TypeBox>
            <TypeBox component="span">{enumText(provider.vehicleType)}</TypeBox>
          </>
        )}
        details={(provider) => <SubscriptionInfo provider={provider} />}
      />
    </>
  )
}
