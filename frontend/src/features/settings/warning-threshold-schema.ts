import { z } from 'zod'

// Zero disables the warning; positive safe integers are user-defined thresholds.
export const warningThresholdSchema = z.number().int().min(0).max(Number.MAX_SAFE_INTEGER)
