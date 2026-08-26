import {
  DndContext,
  DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from '@dnd-kit/core'
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
} from '@dnd-kit/sortable'
import {
  VerticalAlignBottomRounded,
  VerticalAlignTopRounded,
} from '@mui/icons-material'
import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  List,
  ListItem,
  TextField,
  styled,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import yaml from 'js-yaml'
import {
  startTransition,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import { BaseSearchBox, MonacoEditor, VirtualList } from '@/components/base'
import { ProxyItem } from '@/components/profile/proxy-item'
import { readProfileFile, saveProfileFile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useThemeMode } from '@/services/states'
import type { MonacoEditorInstance } from '@/types/monaco'
import getSystem from '@/utils/get-system'
import parseUri from '@/utils/uri-parser'
import { parseYamlSafe } from '@/utils/yaml'

interface Props {
  profileUid: string
  property: string
  open: boolean
  onClose: () => void
  onSave?: (prev?: string, curr?: string) => void
}

export const ProxiesEditorViewer = (props: Props) => {
  const { profileUid, property, open, onClose, onSave } = props
  const { t } = useTranslation()
  const themeMode = useThemeMode()
  const editorRef = useRef<MonacoEditorInstance | null>(null)
  const [prevData, setPrevData] = useState('')
  const [currData, setCurrData] = useState('')
  const [visualization, setVisualization] = useState(true)
  const [match, setMatch] = useState(() => (_: string) => true)
  const [proxyUri, setProxyUri] = useState<string>('')

  const [proxyList, setProxyList] = useState<IProxyConfig[]>([])
  const [prependSeq, setPrependSeq] = useState<IProxyConfig[]>([])
  const [appendSeq, setAppendSeq] = useState<IProxyConfig[]>([])
  const [deleteSeq, setDeleteSeq] = useState<string[]>([])
  const hasLoadedSeqConfigRef = useRef(false)

  // clod: имя ноды служит и ключом React, и идентификатором в сортировке.
  // Нода без имени (вставили в текстовом режиме кусок конфига без `name`)
  // роняла весь экран, поэтому такие ноды не показываем вовсе.
  const named = (proxy: IProxyConfig) => Boolean(proxy?.name)

  const filteredPrependSeq = useMemo(
    () => prependSeq.filter((proxy) => named(proxy) && match(proxy.name)),
    [prependSeq, match],
  )
  const filteredProxyList = useMemo(
    () => proxyList.filter((proxy) => named(proxy) && match(proxy.name)),
    [proxyList, match],
  )
  const filteredAppendSeq = useMemo(
    () => appendSeq.filter((proxy) => named(proxy) && match(proxy.name)),
    [appendSeq, match],
  )

  const renderItem = (index: number): React.ReactNode => {
    const shift = filteredPrependSeq.length > 0 ? 1 : 0
    if (filteredPrependSeq.length > 0 && index === 0) {
      return (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onPrependDragEnd}
        >
          <SortableContext
            items={filteredPrependSeq.map((x) => {
              return x.name
            })}
          >
            {filteredPrependSeq.map((item) => {
              return (
                <ProxyItem
                  key={item.name}
                  type="prepend"
                  proxy={item}
                  onDelete={() => {
                    setPrependSeq(
                      prependSeq.filter((v) => v.name !== item.name),
                    )
                  }}
                />
              )
            })}
          </SortableContext>
        </DndContext>
      )
    } else if (index < filteredProxyList.length + shift) {
      const newIndex = index - shift
      return (
        <ProxyItem
          key={filteredProxyList[newIndex].name}
          type={
            deleteSeq.includes(filteredProxyList[newIndex].name)
              ? 'delete'
              : 'original'
          }
          proxy={filteredProxyList[newIndex]}
          onDelete={() => {
            if (deleteSeq.includes(filteredProxyList[newIndex].name)) {
              setDeleteSeq(
                deleteSeq.filter((v) => v !== filteredProxyList[newIndex].name),
              )
            } else {
              setDeleteSeq((prev) => [
                ...prev,
                filteredProxyList[newIndex].name,
              ])
            }
          }}
        />
      )
    } else {
      return (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onAppendDragEnd}
        >
          <SortableContext
            items={filteredAppendSeq.map((x) => {
              return x.name
            })}
          >
            {filteredAppendSeq.map((item) => {
              return (
                <ProxyItem
                  key={item.name}
                  type="append"
                  proxy={item}
                  onDelete={() => {
                    setAppendSeq(appendSeq.filter((v) => v.name !== item.name))
                  }}
                />
              )
            })}
          </SortableContext>
        </DndContext>
      )
    }
  }

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  )
  const onPrependDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event
    if (over) {
      if (active.id !== over.id) {
        let activeIndex = 0
        let overIndex = 0
        prependSeq.forEach((item, index) => {
          if (item.name === active.id) {
            activeIndex = index
          }
          if (item.name === over.id) {
            overIndex = index
          }
        })

        setPrependSeq(arrayMove(prependSeq, activeIndex, overIndex))
      }
    }
  }
  const onAppendDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event
    if (over) {
      if (active.id !== over.id) {
        let activeIndex = 0
        let overIndex = 0
        appendSeq.forEach((item, index) => {
          if (item.name === active.id) {
            activeIndex = index
          }
          if (item.name === over.id) {
            overIndex = index
          }
        })
        setAppendSeq(arrayMove(appendSeq, activeIndex, overIndex))
      }
    }
  }
  // Оптимизация: асинхронный парсинг по частям, чтобы не блокировать основной поток, setState пакетно после разбора
  const handleParseAsync = (cb: (proxies: IProxyConfig[]) => void) => {
    const proxies: IProxyConfig[] = []
    const names: string[] = []
    let uris: string
    try {
      uris = atob(proxyUri)
    } catch {
      uris = proxyUri
    }
    const lines = uris.trim().split('\n')
    const failed: string[] = []
    let idx = 0
    const batchSize = 50
    let parseTimer: number | undefined

    const parseBatch = () => {
      const end = Math.min(idx + batchSize, lines.length)
      for (; idx < end; idx++) {
        const uri = lines[idx].trim()
        if (!uri) {
          continue
        }
        try {
          const proxy = parseUri(uri)
          if (!names.includes(proxy.name)) {
            proxies.push(proxy)
            names.push(proxy.name)
          }
        } catch (err) {
          console.warn(
            '[ProxiesEditorViewer] parseUri failed for line:',
            uri,
            err,
          )
          failed.push(uri)
          // Не блокируем основной поток
        }
      }
      if (idx < lines.length) {
        parseTimer = window.setTimeout(parseBatch, 0)
      } else {
        if (parseTimer !== undefined) {
          clearTimeout(parseTimer)
          parseTimer = undefined
        }
        if (failed.length > 0) {
          showNotice.error(
            'profiles.page.feedback.notifications.proxyLinksSkipped',
            {
              count: failed.length,
              lines: failed
                .slice(0, 3)
                .map((line) =>
                  line.length > 60 ? `${line.slice(0, 60)}…` : line,
                )
                .join(' · '),
            },
            6000,
          )
        }
        cb(proxies)
      }
    }
    parseBatch()
  }
  const fetchProfile = useCallback(async () => {
    const data = await readProfileFile(profileUid)

    const originProxiesObj = parseYamlSafe(data) as {
      proxies: IProxyConfig[]
    } | null

    setProxyList(originProxiesObj?.proxies || [])
  }, [profileUid])

  const fetchContent = useCallback(async () => {
    hasLoadedSeqConfigRef.current = false
    const data = await readProfileFile(property)
    const obj = parseYamlSafe(data) as ISeqProfileConfig | null | undefined

    setPrevData(data)
    setCurrData(data)

    // clod: файл сломан — показываем его как есть в текстовом режиме и НЕ
    // пускаем сериализатор: иначе он запишет пустой набор поверх правок.
    if (obj === undefined) {
      setVisualization(false)
      showNotice.error(
        t('profiles.page.feedback.notifications.editorBrokenYaml'),
      )
      return
    }

    setPrependSeq(obj?.prepend || [])
    setAppendSeq(obj?.append || [])
    setDeleteSeq(obj?.delete || [])
    hasLoadedSeqConfigRef.current = true
  }, [property, t])

  // clod: текст разбирается обратно ТОЛЬКО при возврате в наглядный режим.
  // Раньше это делал эффект на каждое изменение текста, и недописанная строка
  // бросала исключение прямо из эффекта — экран падал в границу ошибок.
  const handleVisualizationToggle = useCallback(() => {
    if (visualization) {
      setVisualization(false)
      return
    }

    const obj = parseYamlSafe(currData) as ISeqProfileConfig | null | undefined
    if (obj === undefined) {
      hasLoadedSeqConfigRef.current = false
      showNotice.error(
        t('profiles.page.feedback.notifications.editorBrokenYaml'),
      )
      return
    }

    hasLoadedSeqConfigRef.current = true
    startTransition(() => {
      setPrependSeq(obj?.prepend ?? [])
      setAppendSeq(obj?.append ?? [])
      setDeleteSeq(obj?.delete ?? [])
    })
    setVisualization(true)
  }, [currData, t, visualization])

  useEffect(() => {
    if (
      !hasLoadedSeqConfigRef.current ||
      !(prependSeq && appendSeq && deleteSeq)
    ) {
      return
    }

    const serialize = () => {
      if (!hasLoadedSeqConfigRef.current) {
        return
      }

      try {
        setCurrData(
          yaml.dump(
            { prepend: prependSeq, append: appendSeq, delete: deleteSeq },
            { forceQuotes: true },
          ),
        )
      } catch (e) {
        console.warn('[ProxiesEditorViewer] yaml.dump failed:', e)
        // Предотвращаем зависание UI из-за исключения
      }
    }
    let idleId: number | undefined
    let timeoutId: number | undefined
    if (window.requestIdleCallback) {
      idleId = window.requestIdleCallback(serialize)
    } else {
      timeoutId = window.setTimeout(serialize, 0)
    }
    return () => {
      if (idleId !== undefined && window.cancelIdleCallback) {
        window.cancelIdleCallback(idleId)
      }
      if (timeoutId !== undefined) {
        clearTimeout(timeoutId)
      }
    }
  }, [prependSeq, appendSeq, deleteSeq])

  useEffect(() => {
    if (!open) return
    fetchContent()
    fetchProfile()
  }, [fetchContent, fetchProfile, open])

  useEffect(() => {
    return () => {
      editorRef.current?.dispose()
      editorRef.current = null
    }
  }, [])

  const handleSave = useLockFn(async () => {
    try {
      if (!(await saveProfileFile(property, currData))) {
        await fetchContent()
        onClose()
        return
      }
      showNotice.success('shared.feedback.notifications.saved')
      onSave?.(prevData, currData)
      onClose()
    } catch (err) {
      showNotice.error(err)
    }
  })

  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth="xl"
      fullWidth
      disableEnforceFocus={!visualization}
    >
      <DialogTitle>
        {
          <Box sx={{ display: 'flex', justifyContent: 'space-between' }}>
            {t('profiles.modals.proxiesEditor.title')}
            <Box>
              <Button
                variant="contained"
                size="small"
                onClick={handleVisualizationToggle}
              >
                {visualization
                  ? t('shared.editorModes.advanced')
                  : t('shared.editorModes.visualization')}
              </Button>
            </Box>
          </Box>
        }
      </DialogTitle>

      <DialogContent
        sx={{ display: 'flex', width: 'auto', height: 'calc(100vh - 185px)' }}
      >
        {visualization ? (
          <>
            <List
              sx={{
                width: '50%',
                padding: '0 10px',
              }}
            >
              <Box
                sx={{
                  height: 'calc(100% - 80px)',
                  overflowY: 'auto',
                }}
              >
                <Item>
                  <TextField
                    autoComplete="new-password"
                    placeholder={t(
                      'profiles.modals.proxiesEditor.placeholders.multiUri',
                    )}
                    fullWidth
                    rows={9}
                    multiline
                    size="small"
                    onChange={(e) => setProxyUri(e.target.value)}
                  />
                </Item>
              </Box>
              <Item>
                <Button
                  fullWidth
                  variant="contained"
                  startIcon={<VerticalAlignTopRounded />}
                  onClick={() => {
                    handleParseAsync((proxies) => {
                      setPrependSeq((prev) => [...proxies, ...prev])
                    })
                  }}
                >
                  {t('profiles.modals.proxiesEditor.actions.prepend')}
                </Button>
              </Item>
              <Item>
                <Button
                  fullWidth
                  variant="contained"
                  startIcon={<VerticalAlignBottomRounded />}
                  onClick={() => {
                    handleParseAsync((proxies) => {
                      setAppendSeq((prev) => [...prev, ...proxies])
                    })
                  }}
                >
                  {t('profiles.modals.proxiesEditor.actions.append')}
                </Button>
              </Item>
            </List>

            <List
              sx={{
                width: '50%',
                padding: '0 10px',
              }}
            >
              <BaseSearchBox onSearch={(match) => setMatch(() => match)} />
              <VirtualList
                count={
                  filteredProxyList.length +
                  (filteredPrependSeq.length > 0 ? 1 : 0) +
                  (filteredAppendSeq.length > 0 ? 1 : 0)
                }
                estimateSize={56}
                renderItem={renderItem}
                style={{ height: 'calc(100% - 24px)', marginTop: '8px' }}
              />
            </List>
          </>
        ) : (
          <MonacoEditor
            height="100%"
            language="yaml"
            value={currData}
            theme={themeMode === 'light' ? 'light' : 'vs-dark'}
            onMount={(editorInstance) => {
              editorRef.current = editorInstance
            }}
            options={{
              tabSize: 2, // Размер отступа в зависимости от типа языка
              minimap: {
                enabled: document.documentElement.clientWidth >= 1500, // Показывать полосу прокрутки minimap при достаточной ширине
              },
              mouseWheelZoom: true, // Масштабирование колесом мыши при зажатом Ctrl
              quickSuggestions: {
                strings: true, // Подсказки для строк
                comments: true, // Подсказки для комментариев
                other: true, // Подсказки для остального
              },
              padding: {
                top: 33, // Верхний padding, чтобы не перекрывать snippets
              },
              fontFamily: `Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${
                getSystem() === 'windows' ? ', twemoji mozilla' : ''
              }`,
              fontLigatures: false, // Лигатуры
              smoothScrolling: true, // Плавная прокрутка
            }}
            onChange={(value) => setCurrData(value ?? '')}
          />
        )}
      </DialogContent>

      <DialogActions>
        <Button onClick={onClose} variant="outlined">
          {t('shared.actions.cancel')}
        </Button>

        <Button onClick={handleSave} variant="contained">
          {t('shared.actions.save')}
        </Button>
      </DialogActions>
    </Dialog>
  )
}

const Item = styled(ListItem)(() => ({
  padding: '5px 2px',
}))
