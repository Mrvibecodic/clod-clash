import { Box } from '@mui/material'
import { useTranslation } from 'react-i18next'

import type { EditorReadFailure } from '@/utils/editor-save'
import { errorReason } from '@/utils/error-explanation'

export const ReadFailureNote = ({
  failure,
}: {
  failure: EditorReadFailure | null
}) => {
  const { t } = useTranslation()
  if (!failure) return null
  return (
    <Box sx={{ color: 'warning.main', fontSize: 13 }}>
      {t('profiles.modals.editor.messages.partialRead', {
        reason: errorReason(failure.reason, t),
      })}
    </Box>
  )
}
