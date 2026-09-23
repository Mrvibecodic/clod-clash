type EditedProfile = Pick<
  IProfileItem,
  'name' | 'desc' | 'group' | 'url' | 'option'
>

const TEXT_FIELDS = ['name', 'desc', 'group', 'url'] as const

const unset = (key: keyof IProfileOption, value: unknown) =>
  key === 'allow_auto_update' ? value : value || undefined

const OPTION_FIELDS = [
  'user_agent',
  'with_proxy',
  'self_proxy',
  'secure',
  'update_interval',
  'timeout_seconds',
  'danger_accept_invalid_certs',
  'allow_auto_update',
] as const satisfies readonly (keyof IProfileOption)[]

export const profileEditPatch = (
  opened: IProfileItem,
  edited: EditedProfile,
  latestOption?: IProfileOption,
): Partial<IProfileItem> => {
  const patch: Partial<IProfileItem> = {}
  for (const key of TEXT_FIELDS) {
    const value = edited[key]
    if (value !== undefined && value !== (opened[key] ?? '')) patch[key] = value
  }
  const changed = OPTION_FIELDS.filter(
    (key) =>
      unset(key, edited.option?.[key]) !== unset(key, opened.option?.[key]),
  )
  if (changed.length > 0) {
    patch.option = {
      ...latestOption,
      ...Object.fromEntries(changed.map((key) => [key, edited.option?.[key]])),
    }
  }
  return patch
}
