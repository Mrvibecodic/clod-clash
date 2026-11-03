import { useTranslation } from 'react-i18next'

import { BaseSegmented } from '@/components/base'
import { STACKS } from '@/utils/tun-window'

interface Props {
  value?: string
  onChange?: (value: string) => void
}

export const StackModeSwitch = (props: Props) => {
  const { value, onChange } = props
  const { t } = useTranslation()

  return (
    <BaseSegmented
      fullWidth
      value={value?.toLowerCase() ?? ''}
      options={[
        { value: 'auto', label: t('shared.options.fromSubscription') },
        ...STACKS,
      ]}
      onChange={(next) => onChange?.(next)}
    />
  )
}
