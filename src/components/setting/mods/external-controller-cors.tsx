import { Box } from '@mui/material'
import { useLockFn, useRequest } from 'ahooks'
import { forwardRef, useImperativeHandle, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  FormHint,
  FormRow,
  FormSection,
  Switch,
} from '@/components/base'
import { BaseListTiles } from '@/components/base/base-split-chip-editor'
import { useChangeCount } from '@/hooks/use-change-count'
import { useClash } from '@/hooks/use-clash'
import { showNotice } from '@/services/notice-service'

// Пустой список ядро понимает как «пускать всех», поэтому в нём всегда стоит
// источник, который страница в браузере прислать не может; в списке он не
// показывается. Окну сами источники не нужны: оно ходит к ядру через
// локальный сокет, а не по HTTP.
const NO_WEB_PAGE_ORIGIN = 'tauri://localhost'

const originsToSave = (origins: string[]) => [
  ...new Set([
    ...origins.filter((origin) => origin.trim() !== ''),
    NO_WEB_PAGE_ORIGIN,
  ]),
]

const filterBaseOriginsForUI = (origins: string[]) =>
  origins.filter((origin) => origin.trim() !== NO_WEB_PAGE_ORIGIN)

const toComparable = (
  config: { allowPrivateNetwork: boolean; allowOrigins: AllowOriginItem[] },
  draft = '',
) => ({
  allowPrivateNetwork: config.allowPrivateNetwork,
  allowOrigins: [
    ...config.allowOrigins.map((origin) => origin.value),
    ...(draft.trim() ? [draft.trim()] : []),
  ],
})

interface ClashHeaderConfigingRef {
  open: () => void
  close: () => void
}

interface AllowOriginItem {
  key: number
  value: string
}

export const HeaderConfiguration = forwardRef<ClashHeaderConfigingRef>(
  (props, ref) => {
    const { t } = useTranslation()
    const { runtime, patchClash } = useClash()
    const [open, setOpen] = useState(false)

    const lastKeyRef = useRef(0) // Для генерации уникального key

    // Управление состоянием конфига CORS
    const [corsConfig, setCorsConfig] = useState<{
      allowPrivateNetwork: boolean
      allowOrigins: AllowOriginItem[]
    }>(() => {
      const cors = runtime?.['external-controller-cors']
      const origins = cors?.['allow-origins'] ?? []
      return {
        allowPrivateNetwork: cors?.['allow-private-network'] ?? true,
        allowOrigins: filterBaseOriginsForUI(origins).map((origin) => {
          lastKeyRef.current += 1
          return { key: lastKeyRef.current, value: origin }
        }),
      }
    })

    const [draft, setDraft] = useState('')
    const [initial, setInitial] = useState(() => toComparable(corsConfig))
    const current = toComparable(corsConfig, draft)
    const changes = useChangeCount(initial, current)

    // Обработка изменения конфига CORS
    const handleCorsConfigChange = (
      key: 'allowPrivateNetwork' | 'allowOrigins',
      value: boolean | AllowOriginItem[],
    ) => {
      setCorsConfig((prev) => ({
        ...prev,
        [key]: value,
      }))
    }

    // Добавить новый разрешённый источник
    const handleAddOrigin = () => {
      const value = draft.trim()
      if (!value) return
      lastKeyRef.current += 1
      handleCorsConfigChange('allowOrigins', [
        ...corsConfig.allowOrigins,
        { key: lastKeyRef.current, value },
      ])
      setDraft('')
    }

    // Удалить один элемент из списка разрешённых источников
    const handleDeleteOrigin = (index: number) => {
      const newOrigins = [...corsConfig.allowOrigins]
      newOrigins.splice(index, 1)
      handleCorsConfigChange('allowOrigins', newOrigins)
    }

    // Запрос на сохранение конфига
    const { loading, run: saveConfig } = useRequest(
      async () => {
        await patchClash({
          'external-controller-cors': {
            'allow-private-network': current.allowPrivateNetwork,
            'allow-origins': originsToSave(current.allowOrigins),
          },
        })
      },
      {
        manual: true,
        onSuccess: () => {
          setOpen(false)
          showNotice.success('shared.feedback.notifications.common.saveSuccess')
        },
        onError: (err) => {
          showNotice.error(
            'shared.feedback.notifications.common.saveFailed',
            err,
          )
        },
      },
    )

    useImperativeHandle(ref, () => ({
      open: () => {
        const cors = runtime?.['external-controller-cors']
        const origins = cors?.['allow-origins'] ?? []
        lastKeyRef.current = 0
        const opened = {
          allowPrivateNetwork: cors?.['allow-private-network'] ?? true,
          allowOrigins: filterBaseOriginsForUI(origins).map((origin) => {
            lastKeyRef.current += 1
            return { key: lastKeyRef.current, value: origin }
          }),
        }
        setCorsConfig(opened)
        setInitial(toComparable(opened))
        setDraft('')
        setOpen(true)
      },
      close: () => setOpen(false),
    }))

    const handleSave = useLockFn(async () => {
      await saveConfig()
    })

    return (
      <BaseDialog
        open={open}
        title={t('settings.sections.externalCors.title')}
        dividers
        changes={changes}
        onReset={() => {
          lastKeyRef.current = 0
          setCorsConfig({
            allowPrivateNetwork: initial.allowPrivateNetwork,
            allowOrigins: initial.allowOrigins.map((value) => {
              lastKeyRef.current += 1
              return { key: lastKeyRef.current, value }
            }),
          })
          setDraft('')
        }}
        contentSx={{ width: 452 }}
        okBtn={loading ? t('shared.statuses.saving') : t('shared.actions.save')}
        cancelBtn={t('shared.actions.cancel')}
        onClose={() => setOpen(false)}
        onCancel={() => setOpen(false)}
        onOk={handleSave}
      >
        <FormHint sx={{ mb: 0.75 }}>
          {t('settings.sections.externalCors.messages.onlyForPanels')}
        </FormHint>
        <FormRow
          label={t('settings.sections.externalCors.fields.allowPrivateNetwork')}
        >
          <Switch
            edge="end"
            checked={corsConfig.allowPrivateNetwork}
            onChange={(e) =>
              handleCorsConfigChange('allowPrivateNetwork', e.target.checked)
            }
          />
        </FormRow>

        <FormSection
          title={t('settings.sections.externalCors.fields.allowedOrigins')}
          count={corsConfig.allowOrigins.length}
        />
        <Box sx={{ pb: 0.5 }}>
          <BaseListTiles
            items={corsConfig.allowOrigins}
            onRemove={handleDeleteOrigin}
            draft={draft}
            onDraftChange={setDraft}
            onAdd={handleAddOrigin}
            placeholder={t(
              'settings.sections.externalCors.placeholders.origin',
            )}
            addLabel={t('settings.sections.externalCors.actions.add')}
          />
        </Box>
      </BaseDialog>
    )
  },
)
