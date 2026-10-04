import { useCallback, useState } from 'react'

import { useTauriEvent } from '@/hooks/use-listen'

/**
 * clod:chan — ход добавления подписки. Бэкенд при добавлении сначала пробует
 * защищённый канал (до трёх раз, пока сервер молчит), а если у провайдера его
 * нет — загружает обычным путём, и каждый шаг шлёт событием `clod://add-stage`.
 */
export interface AddStage {
  stage: 'checking' | 'retry' | 'plain'
  attempt: number
}

/** Столько раз бэкенд пробует канал (`ADD_CHANNEL_ATTEMPTS` в prfitem.rs). */
const ADD_STAGE_ATTEMPTS = 3

/** Ключ строки и подстановки для шага. */
export const addStageText = (
  stage: AddStage,
): [string, Record<string, number>] => {
  switch (stage.stage) {
    case 'retry':
      return [
        'profiles.modals.profileForm.stages.retry',
        { attempt: stage.attempt, total: ADD_STAGE_ATTEMPTS },
      ]
    case 'plain':
      return ['profiles.modals.profileForm.stages.plain', {}]
    default:
      return ['profiles.modals.profileForm.stages.checking', {}]
  }
}

/** Последний шаг добавления; `reset` — перед новым добавлением. */
export const useAddStage = () => {
  const [stage, setStage] = useState<AddStage>()
  useTauriEvent<AddStage>('clod://add-stage', (event) =>
    setStage(event.payload),
  )
  const reset = useCallback(() => setStage(undefined), [])
  return { stage, reset }
}
