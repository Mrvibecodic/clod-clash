import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { isHotkeyCombo } from './hotkey-combo.ts'

describe('глобальное сочетание клавиш', () => {
  it('модификатор с клавишей или одна F-клавиша — да, остальное — нет', () => {
    const table: [string[], boolean][] = [
      [['CTRL', 'K'], true],
      [['CMD', 'SHIFT', 'P'], true],
      [['F5'], true],
      [['F24'], true],
      [['K'], false],
      [['TAB'], false],
      [['ESCAPE'], false],
      [['CTRL'], false],
      [['CTRL', 'SHIFT'], false],
      [['F25'], false],
      [[], false],
    ]
    for (const [keys, ok] of table) {
      assert.equal(isHotkeyCombo(keys), ok, keys.join('+'))
    }
  })
})
