/**
 * clod: двуязычные ченджлоги. Секция UPDATELOG.md несёт обе части,
 * разделённые маркерами `<!-- lang:en -->` / `<!-- lang:ru -->` (HTML-
 * комментарии — на GitHub невидимы, релиз показывает оба языка подряд).
 * Диалог обновления в приложении показывает часть по языку интерфейса:
 * русский → ru, любой другой → en. Тело без маркеров возвращается как есть.
 */

const LANG_MARKER = /<!--\s*lang:([a-z-]+)\s*-->/gi

export const pickChangelogSection = (
  body: string,
  language: string | undefined,
): string => {
  const sections = new Map<string, string>()
  const matches = [...body.matchAll(LANG_MARKER)]
  if (matches.length === 0) return body

  matches.forEach((match, index) => {
    const start = (match.index ?? 0) + match[0].length
    const end =
      index + 1 < matches.length ? matches[index + 1].index : body.length
    const text = body.slice(start, end).trim()
    if (text) sections.set(match[1].toLowerCase(), text)
  })

  const wantRu = (language ?? '').toLowerCase().startsWith('ru')
  return (
    (wantRu ? sections.get('ru') : undefined) ??
    sections.get('en') ??
    sections.values().next().value ??
    body
  )
}
