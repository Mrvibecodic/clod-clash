export const optionalInt = (raw: string): number | undefined => {
  const value = Number.parseInt(raw.trim(), 10)
  return Number.isNaN(value) ? undefined : value
}
