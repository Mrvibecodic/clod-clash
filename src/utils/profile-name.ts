export const profileDisplayName = (
  profile:
    | Pick<IProfileItem, 'name' | 'custom_name' | 'name_from_panel'>
    | null
    | undefined,
): string | undefined => {
  const custom = profile?.custom_name?.trim() ? profile.custom_name : undefined
  const panel = profile?.name_from_panel ? profile.name : undefined
  if (custom && panel?.trim() && custom !== panel) return `${custom} (${panel})`
  return custom ?? profile?.name
}
