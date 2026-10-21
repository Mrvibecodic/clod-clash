export const splitList = (value: string) =>
  value
    .split(/[,\n;\r]+/)
    .map((item) => item.trim())
    .filter(Boolean)

export const withListDraft = <T extends string | undefined>(
  value: T,
  draft: string,
  separator: string,
): T | string => {
  const added = splitList(draft)
  return added.length > 0
    ? [...splitList(value ?? ''), ...added].join(separator)
    : value
}
