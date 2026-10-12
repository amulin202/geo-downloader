// Development-only visual harness for the actual product warning component.
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource-variable/geist/index.css'
import '../src/index.css'
import { initializeI18n, i18n } from '../src/i18n'
import { Button } from '../src/components/ui/button'
import { StatCard } from '../src/components/layout/stat-card'
import { MemoryBudgetWarning } from '../src/features/download/memory-budget-warning'
import { WarningThresholdInput } from '../src/features/settings/warning-threshold-input'
import { warningThresholdSchema } from '../src/features/settings/warning-threshold-schema'
import { useForm, useWatch } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useTranslation } from 'react-i18next'

await initializeI18n()
await i18n.changeLanguage(new URLSearchParams(location.search).get('lang') || 'zh-CN')
if (new URLSearchParams(location.search).get('theme') === 'dark') document.documentElement.classList.add('dark')

function Harness() {
  const [withinBudget, setWithinBudget] = useState(false)
  const { t } = useTranslation()
  const [receipt, setReceipt] = useState('')
  const form = useForm<{memory_budget_mb: number; tile_count_warning: number}>({
    resolver: zodResolver(z.object({ memory_budget_mb: warningThresholdSchema, tile_count_warning: warningThresholdSchema })),
    defaultValues: {memory_budget_mb: 2048, tile_count_warning: 500000},
  })
  const memory = useWatch({control: form.control, name: 'memory_budget_mb'})
  const tiles = useWatch({control: form.control, name: 'tile_count_warning'})
  return <main className="mx-auto max-w-lg space-y-4 p-6">
    <h1>GeoD · 内存预算提示验收</h1>
    <form className="space-y-4" onSubmit={form.handleSubmit((values) => setReceipt(JSON.stringify(values)))}>
      <WarningThresholdInput id="memory_budget_mb" label={t('settings.fields.memory')} hint={t('settings.memoryHint')}
        value={memory} defaultValue={2048}
        onChange={(v) => form.setValue('memory_budget_mb', v, {shouldDirty: true, shouldValidate: true})}
        error={form.formState.errors.memory_budget_mb?.message} />
      <WarningThresholdInput id="tile_count_warning" label={t('settings.fields.tileCountWarning')} hint={t('settings.tileCountWarningHint')}
        value={tiles} defaultValue={500000}
        onChange={(v) => form.setValue('tile_count_warning', v, {shouldDirty: true, shouldValidate: true})}
        error={form.formState.errors.tile_count_warning?.message} />
      <Button type="submit">保存测试设置</Button>
      <output className="block break-all" aria-live="polite">{receipt}</output>
    </form>
    <StatCard>
      <p>PNG / JPEG · 154,000 tiles</p>
      <MemoryBudgetWarning budgetCheck={{
        allowed: withinBudget || !Number.isFinite(memory) || memory === 0 || memory >= 72188,
        estimated_peak_bytes: 72_188 * 1024 * 1024,
        budget_bytes: memory * 1024 * 1024,
      }} />
    </StatCard>
    <Button variant="outline" onClick={() => setWithinBudget(!withinBudget)}>切换预算内 / 超预算</Button>
  </main>
}
const root = createRoot(document.getElementById('root')!)
root.render(<Harness />)
import.meta.hot?.dispose(() => root.unmount())
