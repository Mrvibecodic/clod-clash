import { useCallback, useMemo } from 'react'

import { useProfiles } from '@/hooks/use-profiles'
import { useVerge } from '@/hooks/use-verge'
import { applyWindowSizeForMode, saveWindowSizeForMode } from '@/services/cmds'

/**
 * Which interface the user gets, and who decided.
 *
 * Three sources, in order:
 *  1. the user's own choice, stored in `verge.simple_mode`;
 *  2. the provider's `clod-simple-mode` header, stored on the profile;
 *  3. the application default, which is the simple interface.
 *
 * The user always wins: a provider can suggest a mode for customers who never
 * opened the settings, but it cannot take the advanced mode away from someone
 * who asked for it.
 */
export const useSimpleMode = () => {
  const { verge, patchVerge } = useVerge()
  const { current } = useProfiles()

  const userChoice = verge?.simple_mode
  const providerChoice = current?.simple_mode

  const simpleMode = useMemo(
    () => userChoice ?? providerChoice ?? true,
    [userChoice, providerChoice],
  )

  const setSimpleMode = useCallback(
    async (enabled: boolean) => {
      // Each mode remembers its own window size: store the size of the mode
      // we are leaving, then resize for the one we are entering. Sizing is
      // cosmetic — a failure must never block the actual mode switch.
      if (enabled !== simpleMode) {
        await saveWindowSizeForMode(simpleMode).catch(() => {})
      }
      await patchVerge({ simple_mode: enabled })
      if (enabled !== simpleMode) {
        await applyWindowSizeForMode(enabled).catch(() => {})
      }
    },
    [patchVerge, simpleMode],
  )

  return {
    /** The mode actually in effect. */
    simpleMode,
    /** True while the provider's preference is what decides. */
    isProviderChoice: userChoice === undefined && providerChoice !== undefined,
    setSimpleMode,
  }
}
