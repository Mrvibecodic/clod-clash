const MODIFIERS = new Set(['CTRL', 'ALT', 'OPTION', 'SHIFT', 'CMD'])

/**
 * Годится ли набор клавиш в глобальное сочетание: модификатор и обычная
 * клавиша или одна F-клавиша. Одиночная буква, Tab или Esc отняли бы клавишу
 * у всех программ.
 */
export const isHotkeyCombo = (keys: readonly string[]) => {
  const plain = keys.filter((key) => !MODIFIERS.has(key))
  if (plain.length === 0) return false
  if (plain.length < keys.length) return true
  return plain.length === 1 && /^F([1-9]|1\d|2[0-4])$/.test(plain[0])
}
