import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { themeModeOf } from './theme-mode.ts'

describe('themeModeOf', () => {
  it('без ключа в настройках — как в системе', () => {
    assert.equal(
      themeModeOf({ theme_mode: null } as unknown as IVergeConfig),
      'system',
    )
    assert.equal(themeModeOf({}), 'system')
  })

  it('выбранная тема остаётся', () => {
    assert.equal(themeModeOf({ theme_mode: 'dark' }), 'dark')
    assert.equal(themeModeOf({ theme_mode: 'light' }), 'light')
  })

  it('настройки ещё не прочитаны — темы нет', () => {
    assert.equal(themeModeOf(undefined), undefined)
    assert.equal(themeModeOf(null), undefined)
  })
})
