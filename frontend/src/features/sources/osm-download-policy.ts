export const OSM_STANDARD_SOURCE_ID = 'osm'
export const OSM_TILE_USAGE_POLICY_URL =
  'https://operations.osmfoundation.org/policies/tiles/'
export const OSM_COPYRIGHT_URL = 'https://www.openstreetmap.org/copyright'
export const OSM_DOWNLOAD_POLICY_EVENT = 'geod:osm-download-policy'

export type OsmDownloadPolicyDecision = 'continue' | 'switch' | 'cancel'

export function isOsmOfflineBlocked(sourceId: string, sourceUrl?: string | null): boolean {
  if (!sourceUrl) return sourceId === OSM_STANDARD_SOURCE_ID
  try {
    const host = new URL(sourceUrl.replaceAll('{s}', 'a')).hostname
      .toLowerCase().replace(/\.+$/, '')
    return host === 'tile.openstreetmap.org' || host.endsWith('.tile.openstreetmap.org')
  } catch {
    return sourceId === OSM_STANDARD_SOURCE_ID
  }
}

type OsmDownloadPolicyRequest = {
  resolve: (decision: Exclude<OsmDownloadPolicyDecision, 'continue'>) => void
}

let pendingDecision: Promise<OsmDownloadPolicyDecision> | null = null

export function requestOsmDownloadApproval(
  sourceId: string,
  sourceUrl?: string | null,
): Promise<OsmDownloadPolicyDecision> {
  if (!isOsmOfflineBlocked(sourceId, sourceUrl)) {
    return Promise.resolve('continue')
  }
  if (pendingDecision) return pendingDecision

  let finishRequest: (decision: OsmDownloadPolicyDecision) => void = () => undefined
  const decisionPromise = new Promise<OsmDownloadPolicyDecision>((resolve) => {
    const finish = (decision: OsmDownloadPolicyDecision) => {
      pendingDecision = null
      // A stale dialog/event listener cannot re-enable the former bypass.
      resolve(decision === 'switch' ? 'switch' : 'cancel')
    }
    finishRequest = finish
  })
  pendingDecision = decisionPromise
  window.dispatchEvent(
    new CustomEvent<OsmDownloadPolicyRequest>(OSM_DOWNLOAD_POLICY_EVENT, {
      detail: { resolve: finishRequest },
    }),
  )
  return decisionPromise
}

export function focusImagerySourcePicker(): void {
  window.setTimeout(() => {
    const trigger = document.querySelector<HTMLElement>(
      '[data-agent-target="imagery-source-select"]',
    )
    trigger?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    trigger?.focus()
    trigger?.click()
  }, 120)
}

export type { OsmDownloadPolicyRequest }
