# Proposal: CPU history for the dashboard API

Status: **design proposal, not landed.** The owner asked for CPU history on 2026-11-26 (Night Orbit design decision 2). This file gives the numbers the build team needs. Landing it in `docs/dashboard-api.md` and `ui/src/api/types.ts` is a product change, which this design thread does not make; it goes through the normal v1 build process.

Prerequisite met: #362 (merged 2026-11-26) added one background sampler per dashboard that refreshes CPU and memory every second. History only needs to keep what that sampler already produces.

## Sampling and retention (arxd)
- The #362 sampler already refreshes every second. arxd folds those readings into a **60 s mean** per minute (a minute with fewer than 30 readings is null).
- Retention: a **24 h ring buffer** of 1,440 one-minute values per host, held in memory. After an arxd restart the earlier minutes are missing (null); nothing is back-filled.
- A minute with no sample (arxd down, host asleep or offline) is **null**. It is never written as 0.

## Endpoints
### `GET /api/v1/hosts/{id}/cpu-history?range=1h|6h|24h` → `CpuHistory`
### `GET /api/v1/hosts/cpu-history?range=24h` → `{"series": CpuHistory[]}` (one per host, ≤ 256; the board loads this once)

| range | bucket | points |
|---|---|---|
| `1h` | 60 s | 60 |
| `6h` | 5 min | 72 |
| `24h` | 10 min | 144 |

```json
{
  "hostId": "atlas",
  "range": "24h",
  "bucketSeconds": 600,
  "from": "2026-11-25T10:20:00Z",
  "to": "2026-11-26T10:20:00Z",
  "samples": [38.2, 41.9, null, 35.0, "…"]
}
```

- `samples[i]` covers `[from + i·bucket, from + (i+1)·bucket)`. Buckets are aligned to the bucket size in UTC.
- A bucket's value is the **mean of the minutes present in it**, rounded to 0.1. If fewer than half of its minutes are present, the bucket is **null**.
- The last bucket may be partial; it uses the same half-coverage rule.
- Unknown `range` → `400 invalid_request`. Unknown host → `404`. An offline host still answers from the server's last-known buffer if it has one; otherwise all null.

## TypeScript (`types.ts`)
```ts
export type CpuHistoryRange = '1h' | '6h' | '24h';
export interface CpuHistory {
  hostId: string;
  range: CpuHistoryRange;
  bucketSeconds: 60 | 300 | 600;
  from: string;              // ISO 8601, first bucket start
  to: string;                // ISO 8601, end of last bucket
  samples: (number | null)[]; // 0–100, one decimal; null = not sampled
}
// ArxApi
getCpuHistory(hostId: string, range: CpuHistoryRange): Promise<CpuHistory>;
listCpuHistory(range: '24h'): Promise<CpuHistory[]>;
```

## Also fixes a #362 P3
The ROADMAP notes that a stalled refresh leaves old figures readable with no sample age. The history buffer gives the dashboard one: if the newest minute is null or older than 2 minutes, "CPU now" shows "usage unavailable" with "last sample N min ago".

## How the design uses it
- **Trace:** one line per host. A null bucket breaks the line, and the gap is hatched. Values are never interpolated across a gap.
- **Time window** (two handles on the host page): selects whole buckets. avg and peak are computed over the non-null buckets in the window. If any bucket in the window is null, the figures read **"avg ≥ partial"** with the count ("avg 54% · 3 of 24 buckets missing"), and no figure is shown when every bucket is null.
- **CPU now:** stays the separate `cpu.usedPercent` sample. It's shown as "usage unavailable" (hatched) when null, even if history exists.
- **Refresh:** the board polls `listCpuHistory('24h')` every 60 s; the host page polls its current range every 60 s (1h range: every 30 s).

## Cost
1,440 values × 4 bytes ≈ 6 KiB per host in memory. A 24 h board response for 256 hosts is about 256 × 144 numbers ≈ 200 KiB of JSON, uncompressed.
