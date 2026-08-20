import CloseRoundedIcon from '@mui/icons-material/CloseRounded'
import ExpandMoreRoundedIcon from '@mui/icons-material/ExpandMoreRounded'
import InfoOutlinedIcon from '@mui/icons-material/InfoOutlined'
import PercentRoundedIcon from '@mui/icons-material/PercentRounded'
import { Alert, Box, Button, IconButton } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BannerText, BaseDialog } from '@/components/base'
import { patchProfile, openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

interface Props {
  profile: IProfileItem
  onChanged: () => Promise<unknown> | void
}

const PROMO_CLAMP_LINES = 5

export const ProviderBanners = ({ profile, onChanged }: Props) => {
  const { t } = useTranslation()

  const [promoNode, setPromoNode] = useState<HTMLElement | null>(null)
  const [promoClipped, setPromoClipped] = useState(false)
  const [promoOpen, setPromoOpen] = useState(false)

  const openLink = useCallback(async (url?: string) => {
    if (!url) return
    try {
      await openWebUrl(url)
    } catch (error) {
      showNotice.error(error)
    }
  }, [])

  const dismissPromo = useLockFn(async () => {
    if (!profile.uid) return
    try {
      await patchProfile(profile.uid, { promo_seen: true })
      await onChanged()
    } catch (error) {
      showNotice.error(error)
    }
  })

  const showPromo = Boolean(profile.promo) && !profile.promo_seen

  useEffect(() => {
    if (!promoNode) return
    const observer = new ResizeObserver(() =>
      setPromoClipped(promoNode.scrollHeight > promoNode.clientHeight + 1),
    )
    observer.observe(promoNode)
    return () => observer.disconnect()
  }, [promoNode, profile.promo])

  return (
    <>
      {showPromo ? (
        <Alert
          severity="info"
          icon={<PercentRoundedIcon fontSize="inherit" />}
          onClick={() => void openLink(profile.promo_url)}
          sx={{
            whiteSpace: 'pre-line',
            cursor: profile.promo_url ? 'pointer' : 'default',
            border: 1,
            borderColor: 'primary.main',
            borderRadius: '12px',
            boxShadow: 'var(--card-shadow)',
          }}
          action={
            <IconButton
              size="small"
              aria-label={t('shared.actions.close')}
              onClick={(event) => {
                event.stopPropagation()
                void dismissPromo()
              }}
            >
              <CloseRoundedIcon fontSize="small" />
            </IconButton>
          }
        >
          <Box
            ref={setPromoNode}
            sx={{
              display: '-webkit-box',
              WebkitBoxOrient: 'vertical',
              WebkitLineClamp: PROMO_CLAMP_LINES,
              overflow: 'hidden',
            }}
          >
            <BannerText text={profile.promo} />
          </Box>
          {promoClipped ? (
            <Button
              size="small"
              endIcon={<ExpandMoreRoundedIcon />}
              sx={{ mt: 0.25, ml: -0.5 }}
              onClick={(event) => {
                event.stopPropagation()
                setPromoOpen(true)
              }}
            >
              {t('home.components.banners.showFull')}
            </Button>
          ) : null}
        </Alert>
      ) : null}

      {profile.announce ? (
        <Alert
          severity="info"
          icon={<InfoOutlinedIcon fontSize="inherit" />}
          onClick={() => void openLink(profile.announce_url)}
          sx={{
            whiteSpace: 'pre-line',
            cursor: profile.announce_url ? 'pointer' : 'default',
            bgcolor: 'action.hover',
            color: 'text.secondary',
            borderRadius: '12px',
            '& .MuiAlert-icon': { color: 'text.secondary' },
          }}
        >
          <BannerText text={profile.announce} />
        </Alert>
      ) : null}

      <BaseDialog
        open={promoOpen}
        title={t('home.components.banners.promoTitle')}
        fullWidth
        maxWidth="sm"
        disableOk
        cancelBtn={t('shared.actions.close')}
        contentSx={{
          whiteSpace: 'pre-line',
          maxHeight: 420,
          overflowY: 'auto',
        }}
        onClose={() => setPromoOpen(false)}
        onCancel={() => setPromoOpen(false)}
      >
        <BannerText text={profile.promo} />
      </BaseDialog>
    </>
  )
}
