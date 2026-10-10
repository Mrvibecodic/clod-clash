import { BaseSegmented } from '@/components/base'

interface Props {
  value?: string
  allowAuto?: boolean
  onChange?: (value: string) => void
}

const STACKS = [
  { value: 'system', label: 'System' },
  { value: 'gvisor', label: 'gVisor' },
  { value: 'mixed', label: 'Mixed' },
  { value: 'mips', label: 'MIPS' },
]

export const StackModeSwitch = (props: Props) => {
  const { value, allowAuto, onChange } = props

  return (
    <BaseSegmented
      fullWidth
      value={value?.toLowerCase() ?? ''}
      options={
        allowAuto ? [{ value: 'auto', label: 'Auto' }, ...STACKS] : STACKS
      }
      onChange={(next) => onChange?.(next)}
    />
  )
}
