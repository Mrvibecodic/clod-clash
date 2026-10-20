import { ToggleButton, ToggleButtonGroup } from '@mui/material'
import { useTranslation } from 'react-i18next'

interface Props {
  visualization: boolean
  disabled: boolean
  onToggle: () => void
}

export const EditorModeSwitch = ({
  visualization,
  disabled,
  onToggle,
}: Props) => {
  const { t } = useTranslation()
  return (
    <ToggleButtonGroup
      exclusive
      size="small"
      color="primary"
      disabled={disabled}
      sx={{ '& .MuiToggleButton-root': { textTransform: 'none' } }}
      value={visualization ? 'form' : 'yaml'}
      onChange={(_, next) => {
        if (next && (next === 'form') !== visualization) onToggle()
      }}
    >
      <ToggleButton value="form" sx={{ px: 1.75, py: 0.5 }}>
        {t('shared.editorModes.form')}
      </ToggleButton>
      <ToggleButton value="yaml" sx={{ px: 1.75, py: 0.5 }}>
        YAML
      </ToggleButton>
    </ToggleButtonGroup>
  )
}
