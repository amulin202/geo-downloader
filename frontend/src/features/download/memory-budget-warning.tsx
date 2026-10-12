import { AlertTriangle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { BudgetCheckResult } from '@/types/api'

export function MemoryBudgetWarning({ budgetCheck }: { budgetCheck?: BudgetCheckResult | null }) {
  const { t, i18n } = useTranslation()
  if (!budgetCheck || budgetCheck.allowed) return null

  const formatMb = (bytes: number) => Math.ceil(bytes / (1024 * 1024)).toLocaleString(i18n.resolvedLanguage)
  return (
    <div role="status" className="mt-1 flex gap-1.5 rounded border border-amber-500/40 bg-amber-500/10 p-1.5 text-amber-700 dark:text-amber-300">
      <AlertTriangle aria-hidden="true" className="size-3.5 shrink-0" />
      <div className="min-w-0 space-y-1">
        <p>{t('download.estimate.memoryWarning')}</p>
        <p>{t('download.estimate.memoryDetails', {
          peak: formatMb(budgetCheck.estimated_peak_bytes),
          budget: formatMb(budgetCheck.budget_bytes),
        })}</p>
        <p>{t('download.estimate.memoryAdvice')}</p>
      </div>
    </div>
  )
}
