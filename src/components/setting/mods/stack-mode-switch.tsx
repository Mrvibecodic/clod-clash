import { Button, ButtonGroup } from '@mui/material'

interface Props {
  value?: string
  allowAuto?: boolean
  onChange?: (value: string) => void
}

export const StackModeSwitch = (props: Props) => {
  const { value, allowAuto, onChange } = props

  return (
    <ButtonGroup size="small" sx={{ my: '4px' }}>
      {allowAuto && (
        <Button
          variant={value?.toLowerCase() === 'auto' ? 'contained' : 'outlined'}
          onClick={() => onChange?.('auto')}
          sx={{ textTransform: 'capitalize' }}
        >
          Auto
        </Button>
      )}
      <Button
        variant={value?.toLowerCase() === 'system' ? 'contained' : 'outlined'}
        onClick={() => onChange?.('system')}
        sx={{ textTransform: 'capitalize' }}
      >
        System
      </Button>
      <Button
        variant={value?.toLowerCase() === 'gvisor' ? 'contained' : 'outlined'}
        onClick={() => onChange?.('gvisor')}
        sx={{ textTransform: 'capitalize' }}
      >
        gVisor
      </Button>
      <Button
        variant={value?.toLowerCase() === 'mixed' ? 'contained' : 'outlined'}
        onClick={() => onChange?.('mixed')}
        sx={{ textTransform: 'capitalize' }}
      >
        Mixed
      </Button>
    </ButtonGroup>
  )
}
