import assert from 'node:assert/strict'
import test from 'node:test'

import {
  OSM_DOWNLOAD_POLICY_EVENT,
  isOsmOfflineBlocked,
  requestOsmDownloadApproval,
  type OsmDownloadPolicyDecision,
  type OsmDownloadPolicyRequest,
} from './osm-download-policy.ts'

test('blocks built-in and custom OSM hosts without blocking lookalikes or other services', () => {
  for (const url of [
    'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
    'https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png',
    'https://A.TILE.OPENSTREETMAP.ORG.:443/1/0/0.png',
    'https://%74ile.openstreetmap.org/1/0/0.png',
  ]) assert.equal(isOsmOfflineBlocked('custom-osm', url), true, url)
  for (const url of [
    'https://tile.openstreetmap.org.example.com/1/0/0.png',
    'https://example.com/tile.openstreetmap.org/1/0/0.png',
    'https://tile.openstreetmap.org@example.com/1/0/0.png',
    'https://overpass-api.de/api/interpreter',
  ]) assert.equal(isOsmOfflineBlocked('osm', url), false, url)
  assert.equal(isOsmOfflineBlocked('osm'), true)
  assert.equal(isOsmOfflineBlocked('arcgis_satellite'), false)
})

test('blocked requests can switch or cancel but never acknowledge a bypass', async () => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: new EventTarget(),
  })
  const decisions: OsmDownloadPolicyDecision[] = ['switch', 'cancel', 'continue']
  let requestCount = 0
  window.addEventListener(OSM_DOWNLOAD_POLICY_EVENT, (event) => {
    requestCount += 1
    const detail = (event as CustomEvent<OsmDownloadPolicyRequest>).detail
    // Simulate an older listener that still emits the former continue action.
    const resolve = detail.resolve as (decision: OsmDownloadPolicyDecision) => void
    resolve(decisions.shift() ?? 'cancel')
  })

  const url = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png'
  assert.equal(await requestOsmDownloadApproval('osm', url), 'switch')
  assert.equal(await requestOsmDownloadApproval('osm', url), 'cancel')
  assert.equal(await requestOsmDownloadApproval('osm', url), 'cancel')
  assert.equal(await requestOsmDownloadApproval('custom-osm', url), 'cancel')
  assert.equal(await requestOsmDownloadApproval('osm', 'https://example.com/tiles'), 'continue')
  assert.equal(requestCount, 4)
})

test('concurrent blocked requests share a single dialog', async () => {
  Object.defineProperty(globalThis, 'window', { configurable: true, value: new EventTarget() })
  let request: OsmDownloadPolicyRequest | undefined
  let count = 0
  window.addEventListener(OSM_DOWNLOAD_POLICY_EVENT, event => {
    request = (event as CustomEvent<OsmDownloadPolicyRequest>).detail
    count++
  })
  const first = requestOsmDownloadApproval('osm')
  const second = requestOsmDownloadApproval('osm')
  assert.equal(count, 1)
  request!.resolve('switch')
  assert.deepEqual(await Promise.all([first, second]), ['switch', 'switch'])
})
