export type DownloadState = 'started' | 'completed' | 'completed_with_gaps' | 'failed' | 'cancelled'
export type DownloadDuration = 'unknown' | 'under_10s' | '10-60s' | '1-5m' | '5-30m' | '30m+'
export type DownloadReason = 'none' | 'cancelled' | 'network' | 'auth' | 'disk' | 'permission' | 'missing_tiles' | 'unknown'
export interface DownloadLifecycleEvent { attempt_id: string; state: DownloadState; duration: DownloadDuration; reason: DownloadReason }
interface TaskObservation { id: string; status: string; error?: string; message?: string }
interface Attempt { attemptId: string; status: string; startedAt: number | null }
export type DownloadTrackerState = [string, Attempt][]
const terminal = new Set(['completed','completed_with_gaps','failed','cancelled'])
const active = new Set(['pending','downloading','paused','pending_decision','merging','processing','exporting','building_pyramid'])
export function durationBucket(ms: number | null): DownloadDuration {
  if (ms === null || !Number.isFinite(ms) || ms < 0) return 'unknown'
  return ms < 10_000 ? 'under_10s' : ms < 60_000 ? '10-60s' : ms < 300_000 ? '1-5m' : ms < 1_800_000 ? '5-30m' : '30m+'
}
export function reasonBucket(state: string, error?: string): DownloadReason {
  if (state === 'cancelled') return 'cancelled'
  if (state === 'completed_with_gaps') return 'missing_tiles'
  if (state !== 'failed') return 'none'
  if (/disk|space|磁盘|空间不足/i.test(error ?? '')) return 'disk'
  if (/permission|denied|权限/i.test(error ?? '')) return 'permission'
  if (/401|403|auth|unauthorized|登录/i.test(error ?? '')) return 'auth'
  if (/timeout|network|connect|超时|网络/i.test(error ?? '')) return 'network'
  return 'unknown'
}

/** IDs remain local except random per-attempt UUIDs. Baseline history is never replayed. */
export class DownloadLifecycleTracker {
  private attempts = new Map<string, Attempt>()
  private initialized = false
  private uuid: () => string
  private now: () => number
  constructor(uuid: () => string, now: () => number, state?: DownloadTrackerState) {
    this.uuid = uuid; this.now = now
    if (state) { this.attempts = new Map(state); this.initialized = true }
  }
  snapshot(): DownloadTrackerState { return [...this.attempts.entries()] }
  observe(tasks: TaskObservation[]): DownloadLifecycleEvent[] {
    const output: DownloadLifecycleEvent[] = []
    for (const task of tasks) {
      if (!terminal.has(task.status) && !active.has(task.status)) continue
      let attempt = this.attempts.get(task.id)
      if (!attempt) {
        attempt = { attemptId: this.uuid(), status: task.status, startedAt: this.initialized ? this.now() : null }
        this.attempts.set(task.id, attempt)
        if (!this.initialized && terminal.has(task.status)) continue
        output.push({ attempt_id: attempt.attemptId, state: 'started', duration: 'unknown', reason: 'none' })
        if (!terminal.has(task.status)) continue
      } else {
        if (attempt.status === task.status) continue
        if (terminal.has(attempt.status) && active.has(task.status)) {
          attempt = { attemptId: this.uuid(), status: task.status, startedAt: this.now() }
          this.attempts.set(task.id, attempt)
          output.push({ attempt_id: attempt.attemptId, state: 'started', duration: 'unknown', reason: 'none' })
          continue
        }
        // Exporting partial results after an already-failed attempt isn't a new download.
        if (terminal.has(attempt.status) && terminal.has(task.status)) { attempt.status = task.status; continue }
        attempt.status = task.status
      }
      if (terminal.has(task.status)) output.push({ attempt_id: attempt.attemptId,
        state: task.status as DownloadState, duration: durationBucket(attempt.startedAt === null ? null : this.now() - attempt.startedAt),
        reason: reasonBucket(task.status, task.error ?? task.message) })
    }
    this.initialized = true
    // Bound local bookkeeping without treating deleted records as cancellations.
    const currentIds = new Set(tasks.map(task => task.id))
    for (const id of this.attempts.keys()) if (!currentIds.has(id)) this.attempts.delete(id)
    return output
  }
}
