export type EmptyState =
  | 'coreDown'
  | 'failed'
  | 'loading'
  | 'belowCoreLevel'
  | 'notFound'
  | 'empty'

export const emptyState = (page: {
  coreDown: boolean
  failed?: boolean
  loading?: boolean
  belowCoreLevel?: boolean
  searching: boolean
  hasData: boolean
}): EmptyState =>
  page.coreDown
    ? 'coreDown'
    : page.failed
      ? 'failed'
      : page.loading
        ? 'loading'
        : page.belowCoreLevel
          ? 'belowCoreLevel'
          : page.searching && page.hasData
            ? 'notFound'
            : 'empty'
