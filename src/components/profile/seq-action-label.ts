export const seqActionLabelKey = (deleted: boolean, sortable: boolean) =>
  deleted
    ? 'rules.modals.editor.list.actions.restore'
    : sortable
      ? 'rules.modals.editor.list.actions.remove'
      : 'rules.modals.editor.list.actions.delete'
