import { useEffect, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, Loader2, Settings as SettingsIcon } from 'lucide-react'
import { toast } from 'sonner'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Separator } from '@/components/ui/separator'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { getSettings, getSystemMemory, saveSettings } from './settings-api'
import type { AppSettings } from '@/types/api'
import { useTranslation } from 'react-i18next'
import { WarningThresholdInput } from './warning-threshold-input'
import { parseTiandituBackups } from './tianditu-tokens'
import { warningThresholdSchema } from './warning-threshold-schema'

const FORMAT_OPTIONS = [
  { value: 'geotiff', label: 'GeoTIFF (.tif)' },
  { value: 'tiles', label: '原始瓦片目录' },
  { value: 'mbtiles', label: 'MBTiles (.mbtiles)' },
  { value: 'gpkg', label: 'GeoPackage (.gpkg)' },
] as const

const settingsSchema = z.object({
  tianditu_token: z.string().trim(),
  tianditu_backup_tokens: z.string().trim(),
  cesium_ion_token: z.string().trim(),
  proxy_enabled: z.boolean(),
  proxy_url: z.string().trim(),
  default_concurrency: z.number().int().min(1).max(100),
  default_zoom: z.number().int().min(0).max(22),
  default_format: z.enum(['geotiff', 'tiles', 'mbtiles', 'gpkg']),
  memory_budget_mb: warningThresholdSchema,
  tile_count_warning: warningThresholdSchema,
  debug_mode: z.boolean(),
  allow_invalid_certs: z.boolean(),
})

type SettingsFormValues = z.infer<typeof settingsSchema>

const DEFAULT_VALUES: SettingsFormValues = {
  tianditu_token: '',
  tianditu_backup_tokens: '',
  cesium_ion_token: '',
  proxy_enabled: false,
  proxy_url: '',
  default_concurrency: 8,
  default_zoom: 10,
  default_format: 'geotiff',
  memory_budget_mb: 2048,
  tile_count_warning: 500000,
  debug_mode: false,
  allow_invalid_certs: false,
}

function fromAppSettings(s: AppSettings | undefined): SettingsFormValues {
  if (!s) return DEFAULT_VALUES
  const fmt = (s.default_format ?? 'geotiff') as SettingsFormValues['default_format']
  const safeFmt = (['geotiff', 'tiles', 'mbtiles', 'gpkg'] as const).includes(fmt)
    ? fmt
    : 'geotiff'
  return {
    tianditu_token: s.tianditu_token ?? '',
    tianditu_backup_tokens: (s.tianditu_backup_tokens ?? []).join('\n'),
    cesium_ion_token: s.cesium_ion_token ?? '',
    proxy_enabled: s.proxy_enabled ?? false,
    proxy_url: s.proxy_url ?? '',
    default_concurrency: s.default_concurrency ?? 8,
    default_zoom: s.default_zoom ?? 10,
    default_format: safeFmt,
    memory_budget_mb: s.memory_budget_mb ?? 2048,
    tile_count_warning: s.tile_count_warning ?? 500000,
    debug_mode: s.debug_mode ?? false,
    allow_invalid_certs: s.allow_invalid_certs ?? false,
  }
}

function toAppSettings(values: SettingsFormValues, base: AppSettings | undefined): AppSettings {
  return {
    ...(base ?? {}),
    tianditu_token: values.tianditu_token.trim() || null,
    tianditu_backup_tokens: parseTiandituBackups(values.tianditu_backup_tokens, values.tianditu_token),
    cesium_ion_token: values.cesium_ion_token.trim() || null,
    proxy_enabled: values.proxy_enabled,
    proxy_url: values.proxy_url.trim(),
    default_concurrency: values.default_concurrency,
    default_zoom: values.default_zoom,
    default_format: values.default_format,
    memory_budget_mb: values.memory_budget_mb,
    tile_count_warning: values.tile_count_warning,
    debug_mode: values.debug_mode,
    allow_invalid_certs: values.allow_invalid_certs,
  }
}

export function SettingsDialog() {
  const [open, setOpen] = useState(false)
  const queryClient = useQueryClient()

  const settingsQuery = useQuery({
    queryKey: ['settings'],
    queryFn: getSettings,
    enabled: open,
  })

  const memoryQuery = useQuery({
    queryKey: ['system-memory'],
    queryFn: getSystemMemory,
    enabled: open,
  })

  const form = useForm<SettingsFormValues>({
    resolver: zodResolver(settingsSchema),
    defaultValues: DEFAULT_VALUES,
  })

  const {
    register,
    handleSubmit,
    reset,
    setValue,
    control,
    formState: { errors, isDirty },
  } = form

  useEffect(() => {
    if (settingsQuery.data) {
      reset(fromAppSettings(settingsQuery.data))
    }
  }, [settingsQuery.data, reset])

  const proxyEnabled = useWatch({ control, name: 'proxy_enabled' })
  const debugMode = useWatch({ control, name: 'debug_mode' })
  const allowInvalidCerts = useWatch({ control, name: 'allow_invalid_certs' })
  const defaultFormat = useWatch({ control, name: 'default_format' })
  const { t } = useTranslation()
  const memoryBudgetMb = useWatch({ control, name: 'memory_budget_mb' })
  const tileCountWarning = useWatch({ control, name: 'tile_count_warning' })

  const mutation = useMutation({
    mutationFn: (values: SettingsFormValues) =>
      saveSettings(toAppSettings(values, settingsQuery.data)),
    onSuccess: (_data, values) => {
      toast.success('设置已保存')
      queryClient.invalidateQueries({ queryKey: ['settings'] })
      reset(values)
      setOpen(false)
    },
    onError: (err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err)
      toast.error(`保存失败：${msg}`)
    },
  })

  const onSubmit = handleSubmit((values) => mutation.mutate(values))

  const loading = settingsQuery.isLoading
  const fetchError = settingsQuery.error

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm" className="gap-2">
          <SettingsIcon className="size-4" />
          设置
        </Button>
      </DialogTrigger>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>应用设置</DialogTitle>
          <DialogDescription>
            修改令牌、代理、并发与默认下载参数，保存后立即生效。
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="flex items-center justify-center py-12 text-muted-foreground">
            <Loader2 className="mr-2 size-4 animate-spin" />
            正在加载设置...
          </div>
        ) : fetchError ? (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive">
            读取设置失败：{fetchError instanceof Error ? fetchError.message : String(fetchError)}
          </div>
        ) : (
          <form onSubmit={onSubmit} className="space-y-6 py-2">
            <section className="space-y-3">
              <h3 className="text-sm font-semibold">访问令牌</h3>
              <div className="space-y-2">
                <Label htmlFor="tianditu_token">天地图 Token</Label>
                <Input
                  id="tianditu_token"
                  placeholder="可选，用于天地图相关图源"
                  autoComplete="off"
                  {...register('tianditu_token')}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="tianditu_backup_tokens">{t('settings.fields.tiandituBackups')}</Label>
                <textarea id="tianditu_backup_tokens" rows={3} autoComplete="off" spellCheck={false}
                  className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm"
                  placeholder={t('settings.fields.tiandituBackupsPlaceholder')}
                  {...register('tianditu_backup_tokens')} />
                <p className="text-xs text-muted-foreground">{t('settings.fields.tiandituBackupsHint')}</p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="cesium_ion_token">Cesium Ion Token</Label>
                <Input
                  id="cesium_ion_token"
                  placeholder="可选，用于 3D Tiles 在线下载"
                  autoComplete="off"
                  {...register('cesium_ion_token')}
                />
              </div>
            </section>

            <Separator />

            <section className="space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-sm font-semibold">网络代理</h3>
                  <p className="text-xs text-muted-foreground">为下载请求启用 HTTP/SOCKS 代理</p>
                </div>
                <Switch
                  checked={proxyEnabled}
                  onCheckedChange={(v) => setValue('proxy_enabled', v, { shouldDirty: true })}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="proxy_url">代理地址</Label>
                <Input
                  id="proxy_url"
                  placeholder="例如 http://127.0.0.1:7890"
                  autoComplete="off"
                  disabled={!proxyEnabled}
                  {...register('proxy_url')}
                />
              </div>
            </section>

            <Separator />

            <section className="space-y-3">
              <h3 className="text-sm font-semibold">默认下载参数</h3>
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="default_concurrency">默认并发数 (1-100)</Label>
                  <Input
                    id="default_concurrency"
                    type="number"
                    min={1}
                    max={100}
                    {...register('default_concurrency', { valueAsNumber: true })}
                  />
                  {errors.default_concurrency && (
                    <p className="text-xs text-destructive">{errors.default_concurrency.message}</p>
                  )}
                </div>
                <div className="space-y-2">
                  <Label htmlFor="default_zoom">默认缩放级别 (0-22)</Label>
                  <Input
                    id="default_zoom"
                    type="number"
                    min={0}
                    max={22}
                    {...register('default_zoom', { valueAsNumber: true })}
                  />
                  {errors.default_zoom && (
                    <p className="text-xs text-destructive">{errors.default_zoom.message}</p>
                  )}
                </div>
                <div className="space-y-2">
                  <Label>默认输出格式</Label>
                  <Select
                    value={defaultFormat}
                    onValueChange={(v) =>
                      setValue('default_format', v as SettingsFormValues['default_format'], {
                        shouldDirty: true,
                      })
                    }
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {FORMAT_OPTIONS.map((opt) => (
                        <SelectItem key={opt.value} value={opt.value}>
                          {opt.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-2">
                  <WarningThresholdInput
                    id="memory_budget_mb"
                    label={t('settings.fields.memory')}
                    hint={t('settings.memoryHint')}
                    value={memoryBudgetMb} defaultValue={2048}
                    onChange={(value) => setValue('memory_budget_mb', value, { shouldDirty: true, shouldValidate: true })}
                    error={errors.memory_budget_mb?.message}
                  />
                  {memoryQuery.data && (
                    <p className="text-xs text-muted-foreground">
                      系统总计 {Math.round(memoryQuery.data.total_mb)} MB / 可用{' '}
                      {Math.round(memoryQuery.data.available_mb)} MB
                      {memoryQuery.data.recommended_budget_mb
                        ? ` · 推荐 ${Math.round(memoryQuery.data.recommended_budget_mb)} MB`
                        : ''}
                    </p>
                  )}
                </div>
                <WarningThresholdInput id="tile_count_warning"
                  label={t('settings.fields.tileCountWarning')}
                  hint={t('settings.tileCountWarningHint')}
                  value={tileCountWarning} defaultValue={500000}
                  onChange={(value) => setValue('tile_count_warning', value, { shouldDirty: true, shouldValidate: true })}
                  error={errors.tile_count_warning?.message} />
              </div>
            </section>

            <Separator />

            <section className="space-y-3">
              <h3 className="text-sm font-semibold">高级</h3>
              <div className="flex items-center justify-between rounded-md border p-3">
                <div className="space-y-0.5">
                  <Label className="text-sm">调试模式</Label>
                  <p className="text-xs text-muted-foreground">
                    保留临时瓦片目录，便于排查下载问题
                  </p>
                </div>
                <Switch
                  checked={debugMode}
                  onCheckedChange={(v) => setValue('debug_mode', v, { shouldDirty: true })}
                />
              </div>
              <div className="flex items-center justify-between rounded-md border p-3">
                <div className="space-y-0.5">
                  <Label className="text-sm">允许无效 HTTPS 证书</Label>
                  <p className="text-xs text-muted-foreground">
                    仅在内网/自签证书环境下开启，存在中间人风险
                  </p>
                </div>
                <Switch
                  checked={allowInvalidCerts}
                  onCheckedChange={(v) =>
                    setValue('allow_invalid_certs', v, { shouldDirty: true })
                  }
                />
              </div>
              {allowInvalidCerts && (
                <div className="flex gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-300">
                  <AlertTriangle className="size-4 shrink-0" />
                  <span>
                    已禁用 HTTPS 证书校验，请确认你信任目标服务器，否则可能被劫持下载内容。
                  </span>
                </div>
              )}
            </section>

            <DialogFooter className="gap-2 sm:gap-2">
              <Button
                type="button"
                variant="ghost"
                onClick={() => setOpen(false)}
                disabled={mutation.isPending}
              >
                取消
              </Button>
              <Button type="submit" disabled={!isDirty || mutation.isPending}>
                {mutation.isPending && <Loader2 className="mr-2 size-4 animate-spin" />}
                保存
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}
