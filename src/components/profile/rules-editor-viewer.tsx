import {
  DndContext,
  DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from '@dnd-kit/core'
import { SortableContext, sortableKeyboardCoordinates } from '@dnd-kit/sortable'
import { AddRounded } from '@mui/icons-material'
import {
  Autocomplete,
  Box,
  Button,
  Checkbox,
  createFilterOptions,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  Stack,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseLoadingOverlay,
  BaseSearchBox,
  MonacoEditor,
  StickyVirtualList,
} from '@/components/base'
import { RuleItem } from '@/components/profile/rule-item'
import { useSeqDocument } from '@/hooks/use-seq-document'
import { readProfileFile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useThemeMode } from '@/services/states'
import type { TranslationKey } from '@/types/generated/i18n-keys'
import type { MonacoEditorInstance } from '@/types/monaco'
import getSystem from '@/utils/get-system'
import { isValidIpCidr } from '@/utils/network'
import { BUILTIN_RULE_POLICIES } from '@/utils/proxy-groups'
import { isBuiltinPolicy, policyName, ruleTypeName } from '@/utils/rule-labels'
import { parseYamlSafe } from '@/utils/yaml'

interface Props {
  groupsUid: string
  mergeUid: string
  profileUid: string
  property: string
  open: boolean
  onClose: () => void
}

const portValidator = (value: string): boolean => {
  return new RegExp(
    '^(?:[1-9]\\d{0,3}|[1-5]\\d{4}|6[0-4]\\d{3}|65[0-4]\\d{2}|655[0-2]\\d|6553[0-5])$',
  ).test(value)
}

type RuleGroup =
  | 'domain'
  | 'ip'
  | 'source'
  | 'port'
  | 'process'
  | 'inbound'
  | 'logic'
  | 'other'

// Порядок — по группам: список типов показывает их с заголовками групп.
const rules: {
  name: string
  group: RuleGroup
  required?: boolean
  example?: string
  noResolve?: boolean
  validator?: (value: string) => boolean
}[] = [
  {
    name: 'DOMAIN',
    group: 'domain',
    example: 'example.com',
  },
  {
    name: 'DOMAIN-SUFFIX',
    group: 'domain',
    example: 'example.com',
  },
  {
    name: 'DOMAIN-KEYWORD',
    group: 'domain',
    example: 'example',
  },
  {
    name: 'DOMAIN-REGEX',
    group: 'domain',
    example: 'example.*',
  },
  {
    name: 'GEOSITE',
    group: 'domain',
    example: 'youtube',
  },
  {
    name: 'IP-CIDR',
    group: 'ip',
    example: '127.0.0.0/8',
    noResolve: true,
    validator: isValidIpCidr,
  },
  {
    name: 'IP-CIDR6',
    group: 'ip',
    example: '2620:0:2d0:200::7/32',
    noResolve: true,
    validator: isValidIpCidr,
  },
  {
    name: 'IP-SUFFIX',
    group: 'ip',
    example: '8.8.8.8/24',
    noResolve: true,
    validator: isValidIpCidr,
  },
  {
    name: 'IP-ASN',
    group: 'ip',
    example: '13335',
    noResolve: true,
    validator: (value) => (+value ? true : false),
  },
  {
    name: 'GEOIP',
    group: 'ip',
    example: 'CN',
    noResolve: true,
  },
  {
    name: 'SRC-IP-CIDR',
    group: 'source',
    example: '192.168.1.201/32',
    validator: isValidIpCidr,
  },
  {
    name: 'SRC-IP-SUFFIX',
    group: 'source',
    example: '192.168.1.201/8',
    validator: isValidIpCidr,
  },
  {
    name: 'SRC-IP-ASN',
    group: 'source',
    example: '9808',
    validator: (value) => (+value ? true : false),
  },
  {
    name: 'SRC-GEOIP',
    group: 'source',
    example: 'CN',
  },
  {
    name: 'SRC-PORT',
    group: 'source',
    example: '7777',
    validator: (value) => portValidator(value),
  },
  {
    name: 'DST-PORT',
    group: 'port',
    example: '80',
    validator: (value) => portValidator(value),
  },
  {
    name: 'IN-PORT',
    group: 'port',
    example: '7897',
    validator: (value) => portValidator(value),
  },
  {
    name: 'NETWORK',
    group: 'port',
    example: 'udp',
    validator: (value) => ['tcp', 'udp'].includes(value),
  },
  {
    name: 'DSCP',
    group: 'port',
    example: '4',
  },
  {
    name: 'PROCESS-NAME',
    group: 'process',
    example: getSystem() === 'windows' ? 'chrome.exe' : 'curl',
  },
  {
    name: 'PROCESS-PATH',
    group: 'process',
    example:
      getSystem() === 'windows'
        ? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe'
        : '/usr/bin/wget',
  },
  {
    name: 'PROCESS-NAME-REGEX',
    group: 'process',
    example: '.*telegram.*',
  },
  {
    name: 'PROCESS-PATH-REGEX',
    group: 'process',
    example:
      getSystem() === 'windows' ? '(?i).*Application\\chrome.*' : '.*bin/wget',
  },
  {
    name: 'UID',
    group: 'process',
    example: '1001',
    validator: (value) => (+value ? true : false),
  },
  {
    name: 'IN-TYPE',
    group: 'inbound',
    example: 'SOCKS/HTTP',
  },
  {
    name: 'IN-USER',
    group: 'inbound',
    example: 'mihomo',
  },
  {
    name: 'IN-NAME',
    group: 'inbound',
    example: 'ss',
  },
  {
    name: 'RULE-SET',
    group: 'logic',
    example: 'providername',
    noResolve: true,
  },
  {
    name: 'SUB-RULE',
    group: 'logic',
    example: '(NETWORK,tcp)',
  },
  {
    name: 'AND',
    group: 'logic',
    example: '((DOMAIN,baidu.com),(NETWORK,UDP))',
  },
  {
    name: 'OR',
    group: 'logic',
    example: '((NETWORK,UDP),(DOMAIN,baidu.com))',
  },
  {
    name: 'NOT',
    group: 'logic',
    example: '((DOMAIN,baidu.com))',
  },
  {
    name: 'MATCH',
    group: 'other',
    required: false,
  },
]

const isRule = (item: unknown) => typeof item === 'string'

type RuleType = (typeof rules)[number]
type Position = 'prepend' | 'append'
type Section = 'prepend' | 'original' | 'append'
// Заголовки разделов — отдельные строки списка: они залипают вверху, пока
// раздел на экране.
type Entry =
  | { kind: 'header'; section: Section }
  | { kind: 'prepend' }
  | { kind: 'rule'; rule: string }
  | { kind: 'append' }

const CodeHint = ({ children }: { children: ReactNode }) => (
  <Box
    component="span"
    sx={{
      flex: 'none',
      fontFamily: 'monospace',
      fontSize: 12,
      color: 'text.secondary',
      whiteSpace: 'nowrap',
    }}
  >
    {children}
  </Box>
)

const Field = ({ label, children }: { label: string; children: ReactNode }) => (
  <Box>
    <Typography
      component="div"
      sx={{ fontSize: 13, fontWeight: 600, mb: 0.75 }}
    >
      {label}
    </Typography>
    {children}
  </Box>
)

const SectionHeader = ({
  title,
  count,
  hint,
}: {
  title: string
  count: number
  hint: string
}) => (
  <Stack
    direction="row"
    sx={{ alignItems: 'baseline', gap: 1, pt: 1.25, pb: 0.5, px: 0.25 }}
  >
    <Typography sx={{ fontSize: 12.5, fontWeight: 700 }}>{title}</Typography>
    <Typography sx={{ fontSize: 12.5, fontWeight: 600 }} color="text.secondary">
      {count}
    </Typography>
    <Typography sx={{ fontSize: 12.5, ml: 'auto' }} color="text.secondary">
      {hint}
    </Typography>
  </Stack>
)

export const RulesEditorViewer = (props: Props) => {
  const { groupsUid, mergeUid, profileUid, property, open, onClose } = props
  const { t } = useTranslation()
  const themeMode = useThemeMode()

  const editorRef = useRef<MonacoEditorInstance | null>(null)

  const {
    loading,
    ready,
    text,
    setText,
    visualization,
    toggleVisualization,
    prependSeq,
    setPrependSeq,
    appendSeq,
    setAppendSeq,
    deleteSeq,
    setDeleteSeq,
    baseline,
    reset,
    save,
  } = useSeqDocument<string>(property, open, { isItem: isRule })
  const [match, setMatch] = useState(() => (_: string) => true)
  const [searching, setSearching] = useState(false)

  const [ruleType, setRuleType] = useState<RuleType>(rules[0])
  const [ruleContent, setRuleContent] = useState('')
  const [noResolve, setNoResolve] = useState(false)
  const [proxyPolicy, setProxyPolicy] = useState<string>(
    BUILTIN_RULE_POLICIES[0],
  )
  const [position, setPosition] = useState<Position>('prepend')
  const [tried, setTried] = useState(false)
  const [proxyPolicyList, setProxyPolicyList] = useState<string[]>([])
  const [ruleList, setRuleList] = useState<string[]>([])
  const [ruleSetList, setRuleSetList] = useState<string[]>([])
  const [subRuleList, setSubRuleList] = useState<string[]>([])

  const filteredPrependSeq = useMemo(
    () => prependSeq.filter((rule) => match(rule)),
    [prependSeq, match],
  )
  const filteredRuleList = useMemo(
    () => ruleList.filter((rule) => match(rule)),
    [ruleList, match],
  )
  const filteredAppendSeq = useMemo(
    () => appendSeq.filter((rule) => match(rule)),
    [appendSeq, match],
  )

  const entries = useMemo(() => {
    const list: Entry[] = []
    if (!searching || filteredPrependSeq.length > 0) {
      list.push({ kind: 'header', section: 'prepend' }, { kind: 'prepend' })
    }
    if (filteredRuleList.length > 0) {
      list.push({ kind: 'header', section: 'original' })
      for (const rule of filteredRuleList) list.push({ kind: 'rule', rule })
    }
    if (filteredAppendSeq.length > 0) {
      list.push({ kind: 'header', section: 'append' }, { kind: 'append' })
    }
    return list
  }, [searching, filteredPrependSeq, filteredRuleList, filteredAppendSeq])

  const filterRuleTypes = useMemo(
    () =>
      createFilterOptions<RuleType>({
        stringify: (option) =>
          `${ruleTypeName(t, option.name)} ${option.name} ${t(
            `rules.modals.editor.ruleGroups.${option.group}`,
          )}`,
      }),
    [t],
  )
  const filterPolicies = useMemo(
    () =>
      createFilterOptions<string>({
        stringify: (option) => `${policyName(t, option)} ${option}`,
      }),
    [t],
  )

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  )
  const reorder = (list: string[], startIndex: number, endIndex: number) => {
    const result = Array.from(list)
    const [removed] = result.splice(startIndex, 1)
    result.splice(endIndex, 0, removed)
    return result
  }
  const onPrependDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event
    if (over) {
      if (active.id !== over.id) {
        const activeIndex = prependSeq.indexOf(active.id.toString())
        const overIndex = prependSeq.indexOf(over.id.toString())
        setPrependSeq(reorder(prependSeq, activeIndex, overIndex))
      }
    }
  }
  const onAppendDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event
    if (over) {
      if (active.id !== over.id) {
        const activeIndex = appendSeq.indexOf(active.id.toString())
        const overIndex = appendSeq.indexOf(over.id.toString())
        setAppendSeq(reorder(appendSeq, activeIndex, overIndex))
      }
    }
  }

  const renderSortable = (
    kind: Position,
    items: string[],
    onDragEnd: (event: DragEndEvent) => void,
    onRemove: (item: string) => void,
  ) => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={onDragEnd}
    >
      <SortableContext items={items}>
        {items.map((item) => (
          <RuleItem
            key={item}
            type={kind}
            ruleRaw={item}
            onDelete={() => onRemove(item)}
          />
        ))}
      </SortableContext>
    </DndContext>
  )

  const sectionCount: Record<Section, number> = {
    prepend: prependSeq.length,
    original: ruleList.length,
    append: appendSeq.length,
  }

  const renderHeader = (entry: Entry) =>
    entry.kind === 'header' ? (
      <SectionHeader
        title={t(`rules.modals.editor.list.sections.${entry.section}`)}
        count={sectionCount[entry.section]}
        hint={t(`rules.modals.editor.list.hints.${entry.section}`)}
      />
    ) : null

  const renderItem = (entry: Entry): ReactNode => {
    if (entry.kind === 'prepend') {
      return filteredPrependSeq.length > 0 ? (
        renderSortable(
          'prepend',
          filteredPrependSeq,
          onPrependDragEnd,
          (item) => setPrependSeq(prependSeq.filter((v) => v !== item)),
        )
      ) : (
        <Box
          sx={{
            my: 0.5,
            px: 1.5,
            py: 1.25,
            border: '1px dashed',
            borderColor: 'divider',
            borderRadius: '10px',
            fontSize: 13,
            color: 'text.secondary',
          }}
        >
          {t('rules.modals.editor.list.emptyPrepend')}
        </Box>
      )
    }
    if (entry.kind === 'append') {
      return renderSortable(
        'append',
        filteredAppendSeq,
        onAppendDragEnd,
        (item) => setAppendSeq(appendSeq.filter((v) => v !== item)),
      )
    }
    if (entry.kind !== 'rule') return null
    const deleted = deleteSeq.includes(entry.rule)
    return (
      <RuleItem
        type={deleted ? 'delete' : 'original'}
        ruleRaw={entry.rule}
        onDelete={() =>
          setDeleteSeq((prev) =>
            deleted
              ? prev.filter((v) => v !== entry.rule)
              : [...prev, entry.rule],
          )
        }
      />
    )
  }

  const fetchProfile = useCallback(async () => {
    const data = await readProfileFile(profileUid) // исходный конфиг-файл
    const groupsData = await readProfileFile(groupsUid) // конфиг-файл groups
    const mergeData = await readProfileFile(mergeUid) // конфиг-файл merge
    const globalMergeData = await readProfileFile('Merge') // конфиг-файл global merge

    const rulesObj = parseYamlSafe(data) as { rules: [] } | null

    const originGroupsObj = parseYamlSafe(data) as {
      'proxy-groups': IProxyGroupConfig[]
    } | null
    const originGroups = originGroupsObj?.['proxy-groups'] || []
    const moreGroupsObj = parseYamlSafe(groupsData) as ISeqProfileConfig | null
    const rawPrependGroups = moreGroupsObj?.['prepend']
    const morePrependGroups = Array.isArray(rawPrependGroups)
      ? (rawPrependGroups as IProxyGroupConfig[])
      : []
    const rawAppendGroups = moreGroupsObj?.['append']
    const moreAppendGroups = Array.isArray(rawAppendGroups)
      ? (rawAppendGroups as IProxyGroupConfig[])
      : []
    const rawDeleteGroups = moreGroupsObj?.['delete']
    const moreDeleteGroups: Array<string | { name: string }> = Array.isArray(
      rawDeleteGroups,
    )
      ? (rawDeleteGroups as Array<string | { name: string }>)
      : []
    const groups = morePrependGroups.concat(
      originGroups.filter((group: any) => {
        if (group.name) {
          return !moreDeleteGroups.includes(group.name)
        } else {
          return !moreDeleteGroups.includes(group)
        }
      }),
      moreAppendGroups,
    )

    const originRuleSetObj = parseYamlSafe(data) as {
      'rule-providers': Record<string, unknown>
    } | null
    const originRuleSet = originRuleSetObj?.['rule-providers'] || {}
    const moreRuleSetObj = parseYamlSafe(mergeData) as {
      'rule-providers': Record<string, unknown>
    } | null
    const moreRuleSet = moreRuleSetObj?.['rule-providers'] || {}
    const globalRuleSetObj = parseYamlSafe(globalMergeData) as {
      'rule-providers': Record<string, unknown>
    } | null
    const globalRuleSet = globalRuleSetObj?.['rule-providers'] || {}
    const ruleSet = Object.assign({}, originRuleSet, moreRuleSet, globalRuleSet)

    const originSubRuleObj = parseYamlSafe(data) as {
      'sub-rules': Record<string, unknown>
    } | null
    const originSubRule = originSubRuleObj?.['sub-rules'] || {}
    const moreSubRuleObj = parseYamlSafe(mergeData) as {
      'sub-rules': Record<string, unknown>
    } | null
    const moreSubRule = moreSubRuleObj?.['sub-rules'] || {}
    const globalSubRuleObj = parseYamlSafe(globalMergeData) as {
      'sub-rules': Record<string, unknown>
    } | null
    const globalSubRule = globalSubRuleObj?.['sub-rules'] || {}
    const subRule = Object.assign({}, originSubRule, moreSubRule, globalSubRule)
    // Группы подписки — первыми: список политик показывает их под своим
    // заголовком, а встроенные — под своим.
    setProxyPolicyList(
      groups
        .map((group: any) => group.name as string)
        .concat(BUILTIN_RULE_POLICIES),
    )
    setRuleSetList(Object.keys(ruleSet))
    setSubRuleList(Object.keys(subRule))
    setRuleList(rulesObj?.rules || [])
  }, [groupsUid, mergeUid, profileUid])

  useEffect(() => {
    if (!open) return
    fetchProfile()
  }, [fetchProfile, open])

  useEffect(() => {
    return () => {
      editorRef.current?.dispose()
      editorRef.current = null
    }
  }, [])

  const required = ruleType.required ?? true
  const value = ruleContent.trim()
  const rawRule = `${ruleType.name}${required && value ? ',' + value : ''},${proxyPolicy}${
    ruleType.noResolve && noResolve ? ',no-resolve' : ''
  }`
  const ruleError =
    required && !value
      ? t('rules.modals.editor.form.validation.conditionRequired')
      : ruleType.validator && !ruleType.validator(value)
        ? t('rules.modals.editor.form.validation.invalidRule')
        : prependSeq.includes(rawRule) || appendSeq.includes(rawRule)
          ? t('rules.modals.editor.form.validation.duplicate')
          : null
  const showError = tried && ruleError !== null

  const addRule = () => {
    setTried(true)
    if (ruleError) return
    if (position === 'prepend') setPrependSeq([rawRule, ...prependSeq])
    else setAppendSeq([...appendSeq, rawRule])
    setRuleContent('')
    setTried(false)
  }

  // Что изменилось относительно файла на диске: добавленное (и возвращённое
  // из удалённых) против убранного (и помеченного на удаление).
  const changes = useMemo(() => {
    if (!baseline) return null
    const missing = (from: string[], to: string[]) =>
      from.filter((item) => !to.includes(item)).length
    return {
      added:
        missing(prependSeq, baseline.prepend) +
        missing(appendSeq, baseline.append) +
        missing(baseline.delete, deleteSeq),
      removed:
        missing(baseline.prepend, prependSeq) +
        missing(baseline.append, appendSeq) +
        missing(deleteSeq, baseline.delete),
      changed:
        JSON.stringify([prependSeq, appendSeq, deleteSeq]) !==
        JSON.stringify([baseline.prepend, baseline.append, baseline.delete]),
    }
  }, [baseline, prependSeq, appendSeq, deleteSeq])
  const unchanged = visualization && changes !== null && !changes.changed

  const handleSave = useLockFn(async () => {
    try {
      if (await save()) onClose()
    } catch (err: any) {
      showNotice.error(err)
    }
  })

  const contentField =
    ruleType.name === 'RULE-SET' || ruleType.name === 'SUB-RULE' ? (
      <Autocomplete
        size="small"
        fullWidth
        options={ruleType.name === 'RULE-SET' ? ruleSetList : subRuleList}
        value={ruleContent || null}
        onChange={(_, next) => next && setRuleContent(next)}
        renderInput={(params) => (
          <TextField
            {...params}
            error={showError}
            placeholder={ruleType.example}
          />
        )}
      />
    ) : (
      <TextField
        autoComplete="new-password"
        size="small"
        fullWidth
        slotProps={{
          htmlInput: {
            autoCorrect: 'off',
            autoCapitalize: 'off',
            spellCheck: false,
          },
        }}
        value={ruleContent}
        error={showError}
        placeholder={ruleType.example}
        onChange={(e) => setRuleContent(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            addRule()
          }
        }}
      />
    )

  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth="xl"
      fullWidth
      disableEnforceFocus={!visualization}
    >
      <DialogTitle>
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          {t('rules.modals.editor.title')}
          <ToggleButtonGroup
            exclusive
            size="small"
            color="primary"
            disabled={!ready}
            sx={{ '& .MuiToggleButton-root': { textTransform: 'none' } }}
            value={visualization ? 'form' : 'yaml'}
            onChange={(_, next) => {
              if (next && (next === 'form') !== visualization) {
                toggleVisualization()
              }
            }}
          >
            <ToggleButton value="form" sx={{ px: 1.75, py: 0.5 }}>
              {t('rules.modals.editor.mode.form')}
            </ToggleButton>
            <ToggleButton value="yaml" sx={{ px: 1.75, py: 0.5 }}>
              YAML
            </ToggleButton>
          </ToggleButtonGroup>
        </Box>
      </DialogTitle>

      <DialogContent
        dividers
        sx={{
          display: 'flex',
          width: 'auto',
          height: 'calc(100vh - 185px)',
          position: 'relative',
          p: 0,
        }}
      >
        <BaseLoadingOverlay isLoading={loading} />
        {visualization ? (
          <>
            <Stack
              sx={{
                width: 360,
                flex: 'none',
                gap: 1.5,
                px: 2.5,
                py: 2,
                borderRight: 1,
                borderColor: 'divider',
                overflowY: 'auto',
                // Полоса прокрутки, если появится, не сдвигает поля.
                scrollbarGutter: 'stable',
              }}
            >
              <Field label={t('rules.modals.editor.form.labels.type')}>
                <Autocomplete
                  size="small"
                  fullWidth
                  disableClearable
                  options={rules}
                  value={ruleType}
                  groupBy={(option) =>
                    t(`rules.modals.editor.ruleGroups.${option.group}`)
                  }
                  getOptionLabel={(option) => ruleTypeName(t, option.name)}
                  isOptionEqualToValue={(option, current) =>
                    option.name === current.name
                  }
                  filterOptions={filterRuleTypes}
                  renderOption={(optionProps, option) => {
                    const { key, ...rest } = optionProps
                    return (
                      <Box component="li" key={key} {...rest} sx={{ gap: 1 }}>
                        <Box sx={{ flex: 1, minWidth: 0 }}>
                          {ruleTypeName(t, option.name)}
                        </Box>
                        <CodeHint>{option.name}</CodeHint>
                      </Box>
                    )
                  }}
                  renderInput={(params) => (
                    <TextField
                      {...params}
                      slotProps={{
                        ...params.slotProps,
                        input: {
                          ...params.slotProps.input,
                          endAdornment: (
                            <>
                              <CodeHint>{ruleType.name}</CodeHint>
                              {params.slotProps.input.endAdornment}
                            </>
                          ),
                        },
                      }}
                    />
                  )}
                  onChange={(_, next) => {
                    setRuleType(next)
                    setTried(false)
                  }}
                />
              </Field>

              {required ? (
                <Field label={t('rules.modals.editor.form.labels.content')}>
                  {contentField}
                  {/* Подсказка и ошибка лежат в одной ячейке, видна одна из
                      них: высота не меняется, поля ниже не сдвигаются. */}
                  <Box
                    sx={{
                      display: 'grid',
                      mt: 0.75,
                      fontSize: 12.5,
                      '& > *': { gridArea: '1 / 1' },
                    }}
                  >
                    <Typography
                      component="div"
                      color="text.secondary"
                      sx={{
                        fontSize: 'inherit',
                        visibility: showError ? 'hidden' : 'visible',
                      }}
                    >
                      {t(
                        `rules.modals.editor.ruleHelp.${ruleType.name}` as TranslationKey,
                      )}{' '}
                      {t('rules.modals.editor.form.example', {
                        example: ruleType.example,
                      })}
                    </Typography>
                    <Typography
                      component="div"
                      color="error"
                      role={showError ? 'alert' : undefined}
                      sx={{
                        fontSize: 'inherit',
                        visibility: showError ? 'visible' : 'hidden',
                      }}
                    >
                      {ruleError}
                    </Typography>
                  </Box>
                </Field>
              ) : (
                <Typography color="text.secondary" sx={{ fontSize: 12.5 }}>
                  {t(
                    `rules.modals.editor.ruleHelp.${ruleType.name}` as TranslationKey,
                  )}
                </Typography>
              )}

              <Field label={t('rules.modals.editor.form.labels.proxyPolicy')}>
                <Autocomplete
                  size="small"
                  fullWidth
                  disableClearable
                  options={proxyPolicyList}
                  value={proxyPolicy}
                  groupBy={(option) =>
                    isBuiltinPolicy(option)
                      ? t('rules.modals.editor.form.policyGroups.builtin')
                      : t('rules.modals.editor.form.policyGroups.groups')
                  }
                  getOptionLabel={(option) => policyName(t, option)}
                  filterOptions={filterPolicies}
                  renderOption={(optionProps, option) => {
                    const { key, ...rest } = optionProps
                    return (
                      <Box component="li" key={key} {...rest} sx={{ gap: 1 }}>
                        <Box sx={{ flex: 1, minWidth: 0 }}>
                          {policyName(t, option)}
                        </Box>
                        <CodeHint>
                          {isBuiltinPolicy(option)
                            ? option
                            : t('rules.modals.editor.form.policyGroups.group')}
                        </CodeHint>
                      </Box>
                    )
                  }}
                  renderInput={(params) => (
                    <TextField
                      {...params}
                      slotProps={{
                        ...params.slotProps,
                        input: {
                          ...params.slotProps.input,
                          endAdornment: (
                            <>
                              <CodeHint>
                                {isBuiltinPolicy(proxyPolicy)
                                  ? proxyPolicy
                                  : t(
                                      'rules.modals.editor.form.policyGroups.group',
                                    )}
                              </CodeHint>
                              {params.slotProps.input.endAdornment}
                            </>
                          ),
                        },
                      }}
                    />
                  )}
                  onChange={(_, next) => setProxyPolicy(next)}
                />
              </Field>

              {ruleType.noResolve ? (
                <FormControlLabel
                  sx={{ mx: 0, gap: 1 }}
                  control={
                    <Checkbox
                      size="small"
                      sx={{ p: 0 }}
                      checked={noResolve}
                      onChange={(_, checked) => setNoResolve(checked)}
                    />
                  }
                  label={
                    <Box component="span" sx={{ fontSize: 13.5 }}>
                      {t('rules.modals.editor.form.toggles.noResolve')}{' '}
                      <CodeHint>no-resolve</CodeHint>
                    </Box>
                  }
                />
              ) : null}

              <Field label={t('rules.modals.editor.form.labels.position')}>
                <ToggleButtonGroup
                  exclusive
                  fullWidth
                  size="small"
                  color="primary"
                  value={position}
                  sx={{ '& .MuiToggleButton-root': { textTransform: 'none' } }}
                  onChange={(_, next: Position | null) =>
                    next && setPosition(next)
                  }
                >
                  <ToggleButton value="prepend">
                    {t('rules.modals.editor.form.position.prepend')}
                  </ToggleButton>
                  <ToggleButton value="append">
                    {t('rules.modals.editor.form.position.append')}
                  </ToggleButton>
                </ToggleButtonGroup>
                <Typography
                  variant="caption"
                  component="div"
                  noWrap
                  color="text.secondary"
                  sx={{ mt: 0.75, fontSize: 12.5 }}
                >
                  {position === 'prepend'
                    ? t('rules.modals.editor.form.position.prependHint')
                    : t('rules.modals.editor.form.position.appendHint')}
                </Typography>
              </Field>

              <Box
                sx={{
                  px: 1.25,
                  py: 0.75,
                  border: '1px dashed',
                  borderColor: 'divider',
                  borderRadius: '8px',
                  fontFamily: 'monospace',
                  fontSize: 12.5,
                  wordBreak: 'break-all',
                }}
              >
                <Typography
                  component="div"
                  color="text.secondary"
                  sx={{ fontSize: 11.5, fontWeight: 600, mb: 0.25 }}
                >
                  {t('rules.modals.editor.form.preview')}
                </Typography>
                {required && !value ? (
                  <>
                    {ruleType.name},
                    <Box component="span" sx={{ color: 'text.disabled' }}>
                      {t('rules.modals.editor.form.previewValue')}
                    </Box>
                    ,{proxyPolicy}
                  </>
                ) : (
                  rawRule
                )}
              </Box>

              <Button
                fullWidth
                variant="contained"
                startIcon={<AddRounded />}
                onClick={addRule}
              >
                {t('rules.modals.editor.form.actions.add')}
              </Button>
            </Stack>

            <Stack sx={{ flex: 1, minWidth: 0, pt: 2 }}>
              <Stack
                direction="row"
                sx={{ alignItems: 'center', gap: 1.25, px: 2.5, pb: 1 }}
              >
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <BaseSearchBox
                    placeholder={t('rules.modals.editor.list.search')}
                    onSearch={(next, state) => {
                      setMatch(() => next)
                      setSearching(state.text.length > 0)
                    }}
                  />
                </Box>
                {/* Ширина не зависит от числа: поле поиска не дёргается. */}
                <Typography
                  color="text.secondary"
                  sx={{
                    flex: 'none',
                    minWidth: 84,
                    textAlign: 'right',
                    fontSize: 12.5,
                    whiteSpace: 'nowrap',
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {t('rules.modals.editor.list.total', {
                    count:
                      prependSeq.length + ruleList.length + appendSeq.length,
                  })}
                </Typography>
              </Stack>
              {entries.length === 0 ? (
                <Typography
                  color="text.secondary"
                  sx={{ textAlign: 'center', fontSize: 13, py: 3 }}
                >
                  {t('rules.modals.editor.list.notFound')}
                </Typography>
              ) : null}
              <Box sx={{ flex: 1, minHeight: 0 }}>
                <StickyVirtualList
                  items={entries}
                  isGroupItem={(entry) => entry.kind === 'header'}
                  getItemKey={(entry, index) =>
                    entry.kind === 'header'
                      ? `header:${entry.section}`
                      : entry.kind === 'rule'
                        ? `rule:${index}:${entry.rule}`
                        : entry.kind
                  }
                  estimateGroupItemHeight={36}
                  estimateItemHeight={52}
                  renderGroupItem={(entry) => (
                    <Box
                      sx={{
                        bgcolor: 'background.paper',
                        backgroundImage: 'var(--Paper-overlay)',
                      }}
                    >
                      {renderHeader(entry)}
                    </Box>
                  )}
                  renderItem={renderItem}
                  style={{
                    boxSizing: 'border-box',
                    padding: '0 20px 12px',
                    scrollbarGutter: 'stable',
                  }}
                />
              </Box>
            </Stack>
          </>
        ) : (
          <MonacoEditor
            height="100%"
            language="yaml"
            value={text}
            theme={themeMode === 'light' ? 'light' : 'vs-dark'}
            onMount={(editorInstance) => {
              editorRef.current = editorInstance
            }}
            options={{
              tabSize: 2, // Размер отступа в зависимости от языка
              minimap: {
                enabled: document.documentElement.clientWidth >= 1500, // Показывать полосу minimap при достаточной ширине
              },
              mouseWheelZoom: true, // Ctrl + колесо мыши регулирует масштаб
              quickSuggestions: {
                strings: true, // Подсказки для строк
                comments: true, // Подсказки для комментариев
                other: true, // Подсказки для остального
              },
              padding: {
                top: 33, // Верхний отступ, чтобы не перекрывать snippets
              },
              fontFamily: `Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${
                getSystem() === 'windows' ? ', twemoji mozilla' : ''
              }`,
              fontLigatures: false, // Лигатуры
              smoothScrolling: true, // Плавная прокрутка
            }}
            onChange={(value) => setText(value ?? '')}
          />
        )}
      </DialogContent>

      <DialogActions sx={{ px: 3, py: 1.5 }}>
        <Box
          sx={{ flex: 1, minWidth: 0, fontSize: 13, color: 'text.secondary' }}
        >
          {visualization && changes ? (
            changes.changed ? (
              <>
                <Box
                  component="span"
                  sx={{ color: 'text.primary', fontWeight: 600 }}
                >
                  {changes.added || changes.removed
                    ? t('rules.modals.editor.footer.unsaved', {
                        added: changes.added,
                        removed: changes.removed,
                      })
                    : t('rules.modals.editor.footer.reordered')}
                </Box>
                <Button size="small" sx={{ ml: 1 }} onClick={reset}>
                  {t('rules.modals.editor.footer.reset')}
                </Button>
              </>
            ) : (
              t('rules.modals.editor.footer.noChanges')
            )
          ) : null}
        </Box>
        <Button onClick={onClose} variant="outlined">
          {t('shared.actions.cancel')}
        </Button>

        <Button
          onClick={handleSave}
          variant="contained"
          disabled={!ready || unchanged}
        >
          {t('shared.actions.save')}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
