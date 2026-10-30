export const backupHistoryNote = ({
  listed,
  total,
}: {
  listed: boolean
  total: number
}) => (!listed ? null : total === 0 ? 'empty' : 'summary')
