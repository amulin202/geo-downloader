import assert from 'node:assert/strict'
import test from 'node:test'
import { warningThresholdSchema } from './warning-threshold-schema.ts'

test('warning thresholds accept unlimited and user values above old ceilings', () => {
  for (const value of [0, 1, 100, 2048, 98304, 500000, 1000000]) {
    assert.equal(warningThresholdSchema.parse(value), value)
  }
})

test('warning thresholds reject negative, fractional, missing and unsafe values', () => {
  for (const value of [-1, 0.5, Number.NaN, Infinity, undefined, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(warningThresholdSchema.safeParse(value).success, false)
  }
})
