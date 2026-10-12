import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'

interface Props {
  id: string
  label: string
  hint: string
  value: number
  defaultValue: number
  onChange: (value: number) => void
  error?: string
}

export function WarningThresholdInput({ id, label, hint, value, defaultValue, onChange, error }: Props) {
  const { t } = useTranslation()
  const lastThreshold = useRef(defaultValue)
  const unlimited = value === 0
  return <div className="space-y-1.5">
    <div className="flex flex-wrap items-center justify-between gap-2">
      <Label htmlFor={id}>{label}</Label>
      <div className="flex items-center gap-2">
        <Label htmlFor={`${id}-unlimited`} className="text-xs text-muted-foreground">{t('settings.unlimited')}</Label>
        <Switch id={`${id}-unlimited`} aria-label={`${label} · ${t('settings.unlimited')}`}
          checked={unlimited} onCheckedChange={(checked) => {
            if (checked && Number.isSafeInteger(value) && value > 0) lastThreshold.current = value
            onChange(checked ? 0 : lastThreshold.current)
          }} />
      </div>
    </div>
    <Input id={id} type="number" min={0} step={1} disabled={unlimited}
      value={unlimited || Number.isNaN(value) ? '' : value}
      placeholder={unlimited ? t('settings.unlimited') : undefined}
      aria-describedby={`${id}-hint`} aria-invalid={Boolean(error)}
      onChange={(event) => onChange(event.target.valueAsNumber)} />
    <p id={`${id}-hint`} className="text-xs text-muted-foreground">{hint}</p>
    {error && <p role="alert" className="text-xs text-destructive">{t('settings.thresholdInvalid')}</p>}
  </div>
}
