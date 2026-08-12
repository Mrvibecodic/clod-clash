import { ExpandMoreRounded } from '@mui/icons-material'
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Box,
  Button,
  FormControl,
  InputAdornment,
  InputLabel,
  LinearProgress,
  MenuItem,
  Select,
  styled,
  TextField,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import type { Ref } from 'react'
import {
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from 'react'
import { Controller, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

import { BaseDialog, Switch } from '@/components/base'
import { useProfiles } from '@/hooks/use-profiles'
import { createProfile, getProfiles, patchProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import parseTraffic from '@/utils/parse-traffic'
import { toUnixSeconds } from '@/utils/subscription-status'
import { version } from '@root/package.json'

import { FileInput } from './file-input'

interface Props {
  onChange: (isActivating?: boolean) => void
}

export interface ProfileViewerRef {
  create: () => void
  edit: (item: IProfileItem) => void
}

/** clod: «создать новую группу» как отдельный пункт списка. */
const NEW_GROUP = '__clod_new_group__'

/** clod: строк несколько, берём случайную — ожидание в пару секунд. */
const LOADING_LINES = [
  'profiles.modals.profileForm.feedback.loading1',
  'profiles.modals.profileForm.feedback.loading2',
  'profiles.modals.profileForm.feedback.loading3',
  'profiles.modals.profileForm.feedback.loading4',
  'profiles.modals.profileForm.feedback.loading5',
] as const

// create or edit the profile
// remote / local
type ProfileViewerProps = Props & { ref?: Ref<ProfileViewerRef> }

/**
 * clod: раньше это была одна форма на десять полей — тип Remote/Local, имя,
 * описание, ссылка, User-Agent, таймаут, интервал и четыре тумблера, — причём
 * одинаковая и для добавления подписки, и для правки существующей. Человеку,
 * который просто купил подписку, нужно ровно одно поле.
 *
 * Теперь добавление идёт в два шага: сначала ссылка (всё прочее свёрнуто в
 * «Дополнительно»), затем — что нашлось по ней, чтобы было видно, ту ли
 * подписку добавил. Правка существующей осталась обычной формой с раскрытыми
 * полями: сюда заходят именно за ними.
 */
export function ProfileViewer({ onChange, ref }: ProfileViewerProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [openType, setOpenType] = useState<'new' | 'edit'>('new')
  const [loading, setLoading] = useState(false)
  const [loadingLine, setLoadingLine] = useState<
    (typeof LOADING_LINES)[number]
  >(LOADING_LINES[0])
  const [errorText, setErrorText] = useState<string>()
  const [added, setAdded] = useState<IProfileItem>()
  const [newGroup, setNewGroup] = useState('')
  const { profiles } = useProfiles()

  // file input
  const fileDataRef = useRef<string | null>(null)

  const { control, watch, setValue, reset, handleSubmit, getValues } =
    useForm<IProfileItem>({
      defaultValues: {
        type: 'remote',
        name: '',
        desc: '',
        url: '',
        group: '',
        option: {
          with_proxy: false,
          self_proxy: false,
        },
      },
    })

  // Уже существующие группы — список для выбора.
  const groups = useMemo(() => {
    const seen = new Set<string>()
    for (const item of profiles?.items ?? []) {
      const group = item.group?.trim()
      if (group) seen.add(group)
    }
    return [...seen].sort((a, b) => a.localeCompare(b))
  }, [profiles?.items])

  const resetState = () => {
    setAdded(undefined)
    setErrorText(undefined)
    setNewGroup('')
  }

  useImperativeHandle(ref, () => ({
    create: () => {
      resetState()
      setOpenType('new')
      setOpen(true)
    },
    edit: (item: IProfileItem) => {
      resetState()
      if (item) {
        Object.entries(item).forEach(([key, value]) => {
          setValue(key as any, value)
        })
      }
      setOpenType('edit')
      setOpen(true)
    },
  }))

  // clod:chan — признак уже включённой защиты: он и запирает переключатель.
  const secureLocked =
    openType === 'edit' &&
    profiles?.items?.some(
      (item) => item.uid === watch('uid') && item.option?.secure === true,
    )

  // clod:chan — отпечаток закреплённого ключа прослойки. Нужен ровно для
  // одного: сверить голосом с тем, что показывает админка провайдера, если
  // возникло подозрение на подмену. Полный ключ показывать незачем.
  const chanFingerprint = profiles?.items
    ?.find((item) => item.uid === watch('uid'))
    ?.option?.chan_pin?.slice(0, 12)

  const selfProxy = watch('option.self_proxy')
  const withProxy = watch('option.with_proxy')

  useEffect(() => {
    if (selfProxy) setValue('option.with_proxy', false)
  }, [selfProxy, setValue])

  useEffect(() => {
    if (withProxy) setValue('option.self_proxy', false)
  }, [setValue, withProxy])

  const handleOk = useLockFn(
    handleSubmit(async (form) => {
      // Второй шаг: подписка уже добавлена, кнопка просто закрывает окно.
      if (added) {
        handleClose()
        return
      }

      setLoading(true)
      setLoadingLine(
        LOADING_LINES[Math.floor(Math.random() * LOADING_LINES.length)],
      )
      setErrorText(undefined)
      try {
        // Базовая валидация
        if (!form.type) throw new Error('`Type` should not be null')
        if (form.type === 'remote' && !form.url) {
          throw new Error(t('profiles.modals.profileForm.errors.urlRequired'))
        }

        // Обработка данных формы
        const option = form.option ? { ...form.option } : undefined
        if (option?.timeout_seconds) {
          option.timeout_seconds = +option.timeout_seconds
        } else if (option) {
          option.timeout_seconds = undefined
        }
        if (option?.update_interval) {
          option.update_interval = +option.update_interval
        } else if (option) {
          option.update_interval = undefined
        }
        if (option?.user_agent === '') {
          option.user_agent = undefined
        }

        const isRemote = form.type === 'remote'
        // clod:panel-name — пустое поле имени у ПОДПИСКИ означает «как назовёт
        // панель», а не «придумай что-нибудь». Раньше отсюда всегда уезжала
        // строка «remote file», бэкенд считал её выбором пользователя
        // (явное имя перебивает `profile-title`) и заголовок панели пропадал:
        // подписка добавлялась болванкой и получала настоящее имя только после
        // ручного «Обновить» — обновление идёт уже без имени. Локальному
        // конфигу заголовков ждать неоткуда, там подстановка остаётся.
        const name = form.name || (isRemote ? undefined : `${form.type} file`)
        const group =
          form.group === NEW_GROUP ? newGroup.trim() : form.group?.trim()
        const item = { ...form, name, group: group || undefined, option }
        const isUpdate = openType === 'edit'

        // Проверяем, является ли конфиг текущим активным
        const isActivating = isUpdate && form.uid === (profiles?.current ?? '')

        // Сохраняем исходные настройки прокси, чтобы восстановить после успешного отката
        const originalOptions = {
          with_proxy: form.option?.with_proxy,
          self_proxy: form.option?.self_proxy,
        }

        // Создание или обновление; локальному конфигу механизм отката не нужен
        if (!isRemote) {
          if (openType === 'new') {
            await createProfile(item, fileDataRef.current)
          } else {
            if (!form.uid) throw new Error('UID not found')
            await patchProfile(form.uid, item)
          }
        } else {
          // Для удалённого конфига используем механизм отката
          try {
            // Пробуем обычную операцию
            if (openType === 'new') {
              await createProfile(item, fileDataRef.current)
            } else {
              if (!form.uid) throw new Error('UID not found')
              await patchProfile(form.uid, item)
            }
          } catch {
            // Первая попытка создания/обновления не удалась, пробуем через собственный прокси
            showNotice.info(
              'profiles.modals.profileForm.feedback.notifications.creationRetry',
            )

            // Конфиг с использованием собственного прокси
            const retryItem = {
              ...item,
              option: {
                ...item.option,
                with_proxy: false,
                self_proxy: true,
              },
            }

            // Повторная попытка через собственный прокси
            if (openType === 'new') {
              await createProfile(retryItem, fileDataRef.current)
            } else {
              if (!form.uid) throw new Error('UID not found')
              await patchProfile(form.uid, retryItem)

              // В режиме редактирования восстанавливаем исходные настройки прокси
              await patchProfile(form.uid, { option: originalOptions })
            }
          }
        }

        fileDataRef.current = null
        onChange(isActivating)

        // clod: при добавлении окно не закрывается — показываем второй шаг с
        // тем, что нашлось по ссылке. При правке закрываем как раньше.
        if (openType === 'new') {
          const fresh = await getProfiles().catch(() => undefined)
          const items = fresh?.items ?? []
          const match =
            [...items]
              .reverse()
              .find((profile) =>
                isRemote ? profile.url === form.url : profile.name === name,
              ) ?? items[items.length - 1]
          setAdded(match ?? ({ name } as IProfileItem))
        } else {
          setOpen(false)
          setTimeout(() => reset(), 500)
        }
      } catch (err) {
        // clod: ошибка живёт в окне, а не улетает тостом — введённое не теряется.
        setErrorText(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
      }
    }),
  )

  const handleClose = () => {
    try {
      setOpen(false)
      fileDataRef.current = null
      setTimeout(() => {
        reset()
        resetState()
      }, 500)
    } catch (e) {
      console.warn('[ProfileViewer] handleClose error:', e)
    }
  }

  const text = {
    fullWidth: true,
    size: 'small',
    margin: 'normal',
    variant: 'outlined',
    autoComplete: 'off',
    autoCorrect: 'off',
  } as const

  const formType = watch('type')
  const groupValue = watch('group')
  // clod: `interval_locked` поднимает бэкенд, когда интервал пришёл из
  // заголовка `profile-update-interval`, а пользователь своего не задавал.
  const intervalLocked = Boolean(watch('interval_locked'))
  const isRemote = formType === 'remote'
  const isLocal = formType === 'local'
  const isNew = openType === 'new'
  // При правке поля нужны сразу: сюда заходят именно за ними.
  const detailsOpen = !isNew

  const title = added
    ? t('profiles.modals.profileForm.title.added')
    : isNew
      ? isLocal
        ? t('profiles.modals.profileForm.title.createLocal')
        : t('profiles.modals.profileForm.title.create')
      : t('profiles.modals.profileForm.title.edit')

  const okBtn = added
    ? t('shared.actions.done')
    : isNew
      ? t('shared.actions.add')
      : t('shared.actions.save')

  const advanced = (
    <>
      <Controller
        name="name"
        control={control}
        render={({ field }) => (
          <TextField
            {...text}
            {...field}
            label={t('profiles.modals.profileForm.fields.displayName')}
          />
        )}
      />

      <Controller
        name="group"
        control={control}
        render={({ field }) => (
          <FormControl size="small" fullWidth sx={{ mt: 1, mb: 1 }}>
            <InputLabel>
              {t('profiles.modals.profileForm.fields.group')}
            </InputLabel>
            <Select
              {...field}
              value={field.value ?? ''}
              label={t('profiles.modals.profileForm.fields.group')}
            >
              <MenuItem value="">
                {t('profiles.modals.profileForm.fields.noGroup')}
              </MenuItem>
              {groups.map((group) => (
                <MenuItem key={group} value={group}>
                  {group}
                </MenuItem>
              ))}
              <MenuItem value={NEW_GROUP}>
                {t('profiles.modals.profileForm.fields.newGroup')}
              </MenuItem>
            </Select>
          </FormControl>
        )}
      />

      {groupValue === NEW_GROUP && (
        <TextField
          {...text}
          autoFocus
          value={newGroup}
          onChange={(event) => setNewGroup(event.target.value)}
          label={t('profiles.modals.profileForm.fields.newGroupName')}
        />
      )}

      <Controller
        name="desc"
        control={control}
        render={({ field }) => (
          <TextField
            {...text}
            {...field}
            label={t('profiles.modals.profileForm.fields.description')}
          />
        )}
      />

      {isRemote && (
        <>
          {/* clod: интервал, заданный провайдером через
              `profile-update-interval`, менять нельзя — иначе обещание
              «задан провайдером» остаётся только в документации. Поле
              выключено и объясняет почему. */}
          <Controller
            name="option.update_interval"
            control={control}
            render={({ field }) => (
              <TextField
                {...text}
                {...field}
                type="number"
                disabled={intervalLocked}
                label={t('profiles.modals.profileForm.fields.updateInterval')}
                helperText={
                  intervalLocked
                    ? t('profiles.modals.profileForm.hints.intervalLocked')
                    : undefined
                }
                slotProps={{
                  input: {
                    endAdornment: (
                      <InputAdornment position="end">
                        {t('shared.units.minutes')}
                      </InputAdornment>
                    ),
                  },
                }}
              />
            )}
          />

          <Controller
            name="option.timeout_seconds"
            control={control}
            render={({ field }) => (
              <TextField
                {...text}
                {...field}
                type="number"
                placeholder="60"
                label={t('profiles.modals.profileForm.fields.httpTimeout')}
                slotProps={{
                  input: {
                    endAdornment: (
                      <InputAdornment position="end">
                        {t('shared.units.seconds')}
                      </InputAdornment>
                    ),
                  },
                }}
              />
            )}
          />

          <Controller
            name="option.user_agent"
            control={control}
            render={({ field }) => (
              <TextField
                {...text}
                {...field}
                placeholder={`ClodClash/${version}`}
                label="User Agent"
              />
            )}
          />

          <Controller
            name="option.allow_auto_update"
            control={control}
            render={({ field }) => (
              <StyledBox>
                <InputLabel>
                  {t('profiles.modals.profileForm.fields.allowAutoUpdate')}
                </InputLabel>
                <Switch checked={field.value} {...field} color="primary" />
              </StyledBox>
            )}
          />

          {/* clod:chan — галочка защищённого канала.
              Поднять можно, снять нельзя: у уже защищённой подписки
              переключатель заблокирован, и вернуть открытый режим можно
              только удалив профиль. Иначе «сними галочку, у тебя не
              работает» становится способом заставить клиента отдать адрес
              подписки посреднику открытым текстом. */}
          <Controller
            name="option.secure"
            control={control}
            render={({ field }) => (
              <StyledBox>
                <InputLabel>
                  {t('profiles.modals.profileForm.fields.secureChannel')}
                </InputLabel>
                <Switch
                  checked={!!field.value}
                  {...field}
                  disabled={!!secureLocked}
                  color="primary"
                />
              </StyledBox>
            )}
          />

          {chanFingerprint && (
            <Box sx={{ mt: -0.5, mb: 1, px: 0.5 }}>
              <Typography variant="caption" color="text.secondary">
                {t('profiles.modals.profileForm.fields.secureKey')}
                {': '}
                <Box component="span" sx={{ fontFamily: 'monospace' }}>
                  {chanFingerprint}
                </Box>
              </Typography>
            </Box>
          )}

          <Controller
            name="option.self_proxy"
            control={control}
            render={({ field }) => (
              <StyledBox>
                <InputLabel>
                  {t('profiles.modals.profileForm.fields.useClashProxy')}
                </InputLabel>
                <Switch checked={field.value} {...field} color="primary" />
              </StyledBox>
            )}
          />

          <Controller
            name="option.with_proxy"
            control={control}
            render={({ field }) => (
              <StyledBox>
                <InputLabel>
                  {t('profiles.modals.profileForm.fields.useSystemProxy')}
                </InputLabel>
                <Switch checked={field.value} {...field} color="primary" />
              </StyledBox>
            )}
          />

          <Controller
            name="option.danger_accept_invalid_certs"
            control={control}
            render={({ field }) => (
              <StyledBox>
                <InputLabel>
                  {t('profiles.modals.profileForm.fields.acceptInvalidCerts')}
                </InputLabel>
                <Switch checked={field.value} {...field} color="primary" />
              </StyledBox>
            )}
          />
        </>
      )}
    </>
  )

  return (
    <BaseDialog
      open={open}
      title={title}
      contentSx={{ width: 375, pb: 0, maxHeight: '80%' }}
      okBtn={okBtn}
      cancelBtn={t('shared.actions.cancel')}
      disableCancel={Boolean(added)}
      onClose={handleClose}
      onCancel={handleClose}
      onOk={handleOk}
      loading={loading}
    >
      {/* Шаг 2: что нашлось по ссылке. */}
      {added ? (
        <Box sx={{ pt: 1 }}>
          <Box
            sx={{
              border: (theme) => `1px solid ${theme.palette.divider}`,
              borderRadius: '12px',
              p: 1.5,
            }}
          >
            <Typography sx={{ fontSize: 16, fontWeight: 600 }} noWrap>
              {added.name}
            </Typography>
            {added.extra ? (
              <>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: 'block', mt: 0.5 }}
                >
                  {parseTraffic(
                    (added.extra.upload ?? 0) + (added.extra.download ?? 0),
                  )}
                  {' / '}
                  {added.extra.total
                    ? parseTraffic(added.extra.total)
                    : t('profiles.components.profileItem.labels.unlimited')}
                </Typography>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: 'block' }}
                >
                  {added.extra.expire
                    ? new Date(
                        toUnixSeconds(added.extra.expire) * 1000,
                      ).toLocaleDateString()
                    : t('profiles.components.profileItem.labels.neverExpires')}
                </Typography>
              </>
            ) : (
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: 'block', mt: 0.5 }}
              >
                {added.url}
              </Typography>
            )}
          </Box>
          <Typography
            sx={{ mt: 1.5, mb: 1, color: 'success.main', fontWeight: 500 }}
          >
            {t('profiles.modals.profileForm.feedback.added')}
          </Typography>
        </Box>
      ) : (
        <>
          {isRemote && (
            <Controller
              name="url"
              control={control}
              render={({ field }) => (
                <TextField
                  {...text}
                  {...field}
                  autoFocus={isNew}
                  multiline
                  error={Boolean(errorText)}
                  label={t(
                    'profiles.modals.profileForm.fields.subscriptionUrl',
                  )}
                />
              )}
            />
          )}

          {isLocal && isNew && (
            <FileInput
              onChange={(file, val) => {
                setValue('name', getValues('name') || file.name)
                fileDataRef.current = val
              }}
            />
          )}

          {loading && (
            <Box sx={{ mt: 1 }}>
              <LinearProgress />
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: 'block', mt: 0.75 }}
              >
                {t(loadingLine)}
              </Typography>
            </Box>
          )}

          {errorText && (
            <Box sx={{ mt: 1 }}>
              <Typography variant="caption" sx={{ color: 'error.main' }}>
                {t('profiles.modals.profileForm.feedback.failed')}
              </Typography>
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: 'block', mt: 0.5, wordBreak: 'break-word' }}
              >
                {errorText}
              </Typography>
            </Box>
          )}

          {isNew && isRemote && !loading && !errorText && (
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ display: 'block', mt: 0.5 }}
            >
              {t('profiles.modals.profileForm.feedback.hint')}
            </Typography>
          )}

          {isNew ? (
            <Accordion
              defaultExpanded={detailsOpen}
              disableGutters
              elevation={0}
              sx={{
                mt: 1.5,
                background: 'transparent',
                '&:before': { display: 'none' },
              }}
            >
              <AccordionSummary
                expandIcon={<ExpandMoreRounded />}
                sx={{ px: 0, minHeight: 36 }}
              >
                <Typography sx={{ fontSize: 13, color: 'primary.main' }}>
                  {t('profiles.modals.profileForm.fields.advanced')}
                </Typography>
              </AccordionSummary>
              <AccordionDetails sx={{ px: 0, pt: 0 }}>
                {advanced}
              </AccordionDetails>
            </Accordion>
          ) : (
            advanced
          )}

          {isNew && (
            <Box sx={{ mt: 1, mb: 1 }}>
              <Button
                size="small"
                sx={{ px: 0 }}
                onClick={() => {
                  setValue('type', isLocal ? 'remote' : 'local')
                  setErrorText(undefined)
                }}
              >
                {isLocal
                  ? t('profiles.modals.profileForm.actions.useLink')
                  : t('profiles.modals.profileForm.actions.useFile')}
              </Button>
            </Box>
          )}
        </>
      )}
    </BaseDialog>
  )
}

const StyledBox = styled(Box)(() => ({
  margin: '8px 0 8px 8px',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
}))
