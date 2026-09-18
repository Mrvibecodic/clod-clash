export const enumText = (value: string | { Unknown: string }): string =>
  typeof value === 'string' ? value : value.Unknown
