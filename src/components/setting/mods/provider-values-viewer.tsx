import { ExpandMoreRounded } from '@mui/icons-material'
import { Box, ButtonBase, Checkbox, Collapse, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import type { Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog, DialogRef } from '@/components/base'
import { getProviderValues, returnProviderValues } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import {
  lineLabel,
  type ProviderValuesGroup,
  type ProviderValuesLine,
  type ProviderValuesView,
  seenLines,
  showsTemplateValue,
  type ValueText,
  valueText,
} from '@/utils/provider-values'

const toggled = <T,>(set: Set<T>, item: T) => {
  const next = new Set(set)
  if (next.has(item)) next.delete(item)
  else next.add(item)
  return next
}

function LineRow({
  line,
  editsReturn,
}: {
  line: ProviderValuesLine
  editsReturn: boolean
}) {
  const { t } = useTranslation()
  const say = (text: ValueText) =>
    text.kind === 'text' ? text.text : t(text.key, text.values)
  const label = lineLabel(line.key)
  const now = say(valueText(line.key, line.now))
  const template = say(valueText(line.key, line.template))
  const becomes =
    line.edited && !editsReturn
      ? t('shared.providerValues.keptByEdits')
      : !showsTemplateValue(line.key) || template === ''
        ? t('shared.options.fromSubscription')
        : t('shared.providerValues.asTemplate', { value: template })

  return (
    <Box
      sx={{
        display: 'grid',
        gridTemplateColumns: 'minmax(0, 1fr) auto minmax(0, 1.1fr)',
        gap: 1,
        alignItems: 'baseline',
        py: 0.5,
        borderBottom: '1px dashed',
        borderColor: 'divider',
        '&:last-of-type': { borderBottom: 0 },
      }}
    >
      <Typography variant="body2" sx={{ overflowWrap: 'anywhere' }}>
        {'i18n' in label ? t(label.i18n) : label.text}
      </Typography>
      <Typography
        variant="body2"
        color="text.secondary"
        sx={{ overflowWrap: 'anywhere' }}
      >
        {now} →
      </Typography>
      <Typography
        variant="body2"
        color="primary"
        sx={{ overflowWrap: 'anywhere' }}
      >
        {becomes}
      </Typography>
    </Box>
  )
}

export function ProviderValuesViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [view, setView] = useState<ProviderValuesView | null>(null)
  const [selected, setSelected] = useState<Set<ProviderValuesGroup>>(
    () => new Set(),
  )
  const [expanded, setExpanded] = useState<Set<ProviderValuesGroup>>(
    () => new Set(),
  )
  const [applying, setApplying] = useState(false)

  useImperativeHandle(ref, () => ({
    open: async () => {
      let fresh: ProviderValuesView | null
      try {
        fresh = await getProviderValues()
      } catch (error) {
        showNotice.error(error)
        return
      }
      if (!fresh) {
        showNotice.error('shared.providerValues.unreadable')
        return
      }
      if (!fresh.groups.length) {
        showNotice.info('shared.providerValues.same')
        return
      }
      setView(fresh)
      setSelected(new Set(fresh.groups.map((view) => view.group)))
      setExpanded(new Set())
      setOpen(true)
    },
    close: () => setOpen(false),
  }))

  const onReturn = useLockFn(async () => {
    if (!view) return
    setApplying(true)
    try {
      const outcome = await returnProviderValues(
        view.uid,
        seenLines(view.groups, selected),
      )
      if (outcome === 'returned') {
        showNotice.success('shared.providerValues.returned')
      } else {
        showNotice.error('shared.providerValues.stale')
      }
      setOpen(false)
    } catch (error) {
      showNotice.error('shared.providerValues.failed', error)
    } finally {
      setApplying(false)
    }
  })

  const groups = view?.groups ?? []
  const editsReturn = selected.has('edits')

  return (
    <BaseDialog
      open={open}
      title={t('shared.providerValues.title')}
      dividers
      contentSx={{ width: 452, px: 1.5 }}
      footerStart={
        <Typography variant="caption" color="text.secondary">
          {t('shared.providerValues.selected', {
            count: selected.size,
            total: groups.length,
          })}
        </Typography>
      }
      okBtn={t('shared.providerValues.ok')}
      cancelBtn={t('shared.actions.cancel')}
      disableOk={selected.size === 0}
      loading={applying}
      onOk={onReturn}
      onCancel={() => setOpen(false)}
      onClose={() => setOpen(false)}
    >
      <Typography variant="body2" color="text.secondary" sx={{ px: 1, pb: 1 }}>
        {t('shared.providerValues.intro', { name: view?.name ?? '' })}
      </Typography>
      {groups.map((group) => {
        const isOpen = expanded.has(group.group)
        const isOn = selected.has(group.group)
        const title = t(`shared.providerValues.groups.${group.group}`)
        return (
          <Box key={group.group}>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
              <Checkbox
                size="small"
                checked={isOn}
                onChange={() => setSelected((now) => toggled(now, group.group))}
                slotProps={{ input: { 'aria-label': title } }}
              />
              <ButtonBase
                aria-expanded={isOpen}
                onClick={() => setExpanded((now) => toggled(now, group.group))}
                sx={{
                  flex: 1,
                  display: 'flex',
                  alignItems: 'center',
                  gap: 1,
                  px: 1,
                  py: 0.75,
                  borderRadius: 1.5,
                  justifyContent: 'flex-start',
                  textAlign: 'left',
                  '&:hover': { bgcolor: 'action.hover' },
                }}
              >
                <Typography sx={{ flex: 1, fontWeight: 600 }} variant="body2">
                  {title}
                </Typography>
                {group.shared ? (
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{
                      px: 1,
                      borderRadius: 999,
                      bgcolor: 'action.hover',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {t('shared.providerValues.shared')}
                  </Typography>
                ) : null}
                <ExpandMoreRounded
                  fontSize="small"
                  sx={{
                    color: 'text.secondary',
                    transition: 'transform 0.2s',
                    transform: isOpen ? 'rotate(180deg)' : 'none',
                  }}
                />
              </ButtonBase>
            </Box>
            {group.group === 'edits' && isOn ? (
              <Typography
                variant="caption"
                color="warning.main"
                component="div"
                sx={{ pl: 5.5, pb: 0.5 }}
              >
                {t('shared.providerValues.editsWarning')}
              </Typography>
            ) : null}
            <Collapse in={isOpen} unmountOnExit>
              <Box sx={{ pl: 5.5, pr: 1, pb: 1, opacity: isOn ? 1 : 0.5 }}>
                {group.lines.map((line) => (
                  <LineRow
                    key={line.key}
                    line={line}
                    editsReturn={editsReturn}
                  />
                ))}
              </Box>
            </Collapse>
          </Box>
        )
      })}
    </BaseDialog>
  )
}
