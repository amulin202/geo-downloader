import assert from 'node:assert/strict'
import test from 'node:test'
import { DownloadLifecycleTracker } from './download-lifecycle.ts'

test('restarting the observer preserves attempt IDs and does not replay terminal outcomes', () => {
  let id = 0
  const tracker = new DownloadLifecycleTracker(() => `random-${++id}`, () => 0)
  const started = tracker.observe([{ id: 'a', status: 'downloading' }])[0]
  const restarted = new DownloadLifecycleTracker(() => `random-${++id}`, () => 50000, JSON.parse(JSON.stringify(tracker.snapshot())))
  assert.equal(restarted.observe([{ id: 'a', status: 'downloading' }]).length, 0)
  assert.equal(restarted.observe([{ id: 'a', status: 'completed' }])[0].attempt_id, started.attempt_id)
  const again = new DownloadLifecycleTracker(() => `random-${++id}`, () => 90000, restarted.snapshot())
  assert.equal(again.observe([{ id: 'a', status: 'completed' }]).length, 0)
})

test('baseline history is suppressed, live completion emitted once without private task fields', () => {
  let time = 0, sequence = 0
  const tracker = new DownloadLifecycleTracker(() => `uuid-${++sequence}`, () => time)
  assert.equal(tracker.observe([{id:'old',status:'completed'}]).length, 0)
  const task = {id:'private-job-path',status:'downloading',message:'https://secret.test/?key=private'}
  const started = tracker.observe([task])
  time = 25_000
  const done = tracker.observe([{...task,status:'completed'}])
  assert.equal(done[0].attempt_id, started[0].attempt_id)
  assert.equal(done[0].duration,'10-60s')
  assert.equal(done[0].state,'completed')
  assert.equal(tracker.observe([{...task,status:'completed'}]).length,0)
  assert.doesNotMatch(JSON.stringify([...started,...done]), /private|secret/)
})
test('resume creates a separate attempt, pause retains attempt, incomplete export is not success', () => {
  let sequence=0
  const tracker=new DownloadLifecycleTracker(()=>`uuid-${++sequence}`,()=>0)
  const started=tracker.observe([{id:'a',status:'downloading'}])[0]
  assert.equal(tracker.observe([{id:'a',status:'paused'}]).length,0)
  const failed=tracker.observe([{id:'a',status:'failed',error:'network timeout password=secret'}])[0]
  assert.equal(failed.reason,'network')
  assert.equal(failed.attempt_id,started.attempt_id)
  const resumed=tracker.observe([{id:'a',status:'downloading'}])[0]
  assert.notEqual(resumed.attempt_id,started.attempt_id)
  assert.equal(tracker.observe([{id:'a',status:'completed_with_gaps'}])[0].reason,'missing_tiles')
})
test('a task that finishes between polls still has an attempt, deletion is not cancellation', () => {
  const tracker=new DownloadLifecycleTracker(()=>crypto.randomUUID(),()=>0)
  tracker.observe([])
  const events=tracker.observe([{id:'fast',status:'cancelled'}])
  assert.deepEqual(events.map(e=>e.state),['started','cancelled'])
  assert.equal(tracker.observe([]).length,0)
})
