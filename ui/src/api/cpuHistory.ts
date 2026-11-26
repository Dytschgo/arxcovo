/** Summarizes the selected buckets. The missing count is part of the display. */
export function summarizeCpuHistory(samples: readonly (number | null)[]): {
  avg: number | null;
  peak: number | null;
  missing: number;
} {
  let sum = 0;
  let present = 0;
  let peak = 0;
  for (const sample of samples) {
    if (sample === null) continue;
    sum += sample;
    present += 1;
    peak = Math.max(peak, sample);
  }
  const missing = samples.length - present;
  return present === 0 ? { avg: null, peak: null, missing } : { avg: sum / present, peak, missing };
}
