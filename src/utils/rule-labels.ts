import type { useTranslation } from 'react-i18next'

import type { TranslationKey } from '@/types/generated/i18n-keys'
import { BUILTIN_RULE_POLICIES } from '@/utils/proxy-groups'

type Translate = ReturnType<typeof useTranslation>['t']

/** Переводы называют тип или политику вместе с кодом в скобках — код рядом
 *  показан отдельно, поэтому из подписи он убирается. */
const withoutCode = (label: string, code: string) =>
  label.endsWith(` (${code})`) ? label.slice(0, -(code.length + 3)) : label

// Тип, которого нет в переводах (из свежего ядра), показывается своим кодом.
export const ruleTypeName = (t: Translate, type: string) => {
  const key = `rules.modals.editor.ruleTypes.${type}` as TranslationKey
  const label = t(key)
  return label === key ? type : withoutCode(label, type)
}

export const isBuiltinPolicy = (policy: string) =>
  BUILTIN_RULE_POLICIES.includes(policy)

export const policyName = (t: Translate, policy: string) =>
  isBuiltinPolicy(policy)
    ? t(`rules.modals.editor.policies.${policy}` as TranslationKey)
    : policy
