export const backupHistoryNote = ({
  notConfigured,
  serverFailed,
  listed,
  total,
}: {
  notConfigured: boolean
  serverFailed: boolean
  listed: boolean
  total: number
}) =>
  notConfigured
    ? 'notConfigured'
    : serverFailed
      ? 'serverFailed'
      : !listed
        ? null
        : total === 0
          ? 'empty'
          : 'summary'
