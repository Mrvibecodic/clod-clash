import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

import {
  changeLanguage,
  resolveLanguage,
  supportedLanguages,
} from '@/services/i18n'

import { useVerge } from './use-verge'

export const useI18n = () => {
  const { i18n } = useTranslation()
  const { patchVerge } = useVerge()

  const switchLanguage = useCallback(
    async (language: string) => {
      const targetLanguage = resolveLanguage(language)

      if (!supportedLanguages.includes(targetLanguage)) {
        console.warn(`Unsupported language: ${language}`)
        return
      }

      if (i18n.language === targetLanguage) {
        return
      }

      try {
        await changeLanguage(targetLanguage)

        if (patchVerge) {
          await patchVerge({ language: targetLanguage })
        }
      } catch (error) {
        console.error('Failed to change language:', error)
      }
    },
    [i18n.language, patchVerge],
  )

  return {
    switchLanguage,
  }
}
