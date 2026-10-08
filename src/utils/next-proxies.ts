const NO_LABELS: IProxyLabels = { proxies: {}, providers: {} }

/** Отпечатки, по которым прочитаны показанные группы и узлы. */
export interface ProxiesStamps {
  /** Отпечаток ядра, по которому прочитаны эти данные. */
  stamp: string
  /** Подписи узлов, по которым они собраны, и их отпечаток. */
  labels: { stamp: string; labels: IProxyLabels }
}

/**
 * Что показывать после ответа `get_proxies_snapshot`. Ядро не ответило,
 * провайдер не отдался или ничего не сменилось — остаётся показанное
 * (`shown`). Подписей в снимке нет — значит, они те же, что у показанного.
 */
export const nextProxies = <
  S extends { labels: { stamp: string; labels?: IProxyLabels } },
  P extends object,
>(
  shown: (P & ProxiesStamps) | undefined,
  answer: { stamp: string; snapshot?: S } | null,
  calc: (snapshot: S, labels: IProxyLabels) => P,
): (P & ProxiesStamps) | undefined => {
  if (!answer?.snapshot) return shown
  const { stamp, snapshot } = answer
  const labels = {
    stamp: snapshot.labels.stamp,
    labels: snapshot.labels.labels ?? shown?.labels.labels ?? NO_LABELS,
  }
  try {
    return { ...calc(snapshot, labels.labels), stamp, labels }
  } catch {
    // Ядро ещё не готово (clod:Э11-05) — остаётся прежний список. Ошибкой
    // это не делаем: она остановила бы опрос SWR, а следующий тик и так
    // спросит снова.
    return shown
  }
}
