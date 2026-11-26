use super::{ApiError, Dashboard, instant_string, json_response, read::validate_id};
use axum::{extract::Query, http::Uri, response::Response};
use serde::{Deserialize, Serialize};

const MINUTE_MS: i64 = 60_000;
const RETENTION_MINUTES: usize = 24 * 60;
const MIN_READINGS: u64 = 30;

#[derive(Clone, Copy, Deserialize, Serialize)]
pub(super) enum Range {
    #[serde(rename = "1h")]
    Hour,
    #[serde(rename = "6h")]
    SixHours,
    #[serde(rename = "24h")]
    Day,
}

impl Range {
    fn dimensions(self) -> (i64, usize) {
        match self {
            Self::Hour => (1, 60),
            Self::SixHours => (5, 72),
            Self::Day => (10, 144),
        }
    }

    pub(super) fn series(
        self,
        host_id: &str,
        now: i64,
        history: &MinuteHistory,
    ) -> Result<CpuHistory, ApiError> {
        let (width, points) = self.dimensions();
        let width_ms = width * MINUTE_MS;
        // End at the bucket containing the most recent completed minute,
        // even if the rest of that bucket is still in progress.
        let now_minute = now.div_euclid(MINUTE_MS);
        let end = (now_minute - 1).div_euclid(width) + 1;
        let start = end - points as i64;
        let samples = (start..end)
            .map(|bucket| history.bucket_mean(bucket * width, width, now_minute))
            .collect();
        Ok(CpuHistory {
            host_id: host_id.to_owned(),
            range: self,
            bucket_seconds: width * 60,
            from: instant_string(start * width_ms)?,
            to: instant_string(end * width_ms)?,
            samples,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CpuHistory {
    host_id: String,
    range: Range,
    bucket_seconds: i64,
    from: String,
    to: String,
    samples: Vec<Option<f64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    range: Range,
}

/// Called after authentication by the shared dashboard read dispatcher.
pub(super) async fn route(
    state: &Dashboard,
    uri: &Uri,
    id: Option<&str>,
) -> Result<Response, ApiError> {
    if let Some(id) = id {
        validate_id(id)?;
    }
    if uri.query().is_some_and(|query| query.len() > 32) {
        return Err(ApiError::INVALID);
    }
    let Query(HistoryQuery { range }) = Query::try_from_uri(uri).map_err(|_| ApiError::INVALID)?;
    if id.is_none() && !matches!(range, Range::Day) {
        return Err(ApiError::INVALID);
    }

    let state = state.clone();
    let id = id.map(str::to_owned);
    tokio::task::spawn_blocking(move || respond(&state, range, id.as_deref()))
        .await
        .map_err(|_| ApiError::UNAVAILABLE)?
}

fn respond(state: &Dashboard, range: Range, id: Option<&str>) -> Result<Response, ApiError> {
    // The dashboard host inventory currently contains one host, so the
    // batch is bounded to one series. Cluster messages carry no CPU history.
    let host_id = state
        .activity
        .host_id()
        .map_err(|_| ApiError::UNAVAILABLE)?;
    if id.is_some_and(|id| id != host_id) {
        return Err(ApiError::NOT_FOUND);
    }
    let series = state
        .host
        .history(range, &host_id, state.clock.now().utc_ms)?;
    if id.is_some() {
        json_response(series)
    } else {
        #[derive(Serialize)]
        struct Batch {
            series: [CpuHistory; 1],
        }

        json_response(Batch { series: [series] })
    }
}

struct Minute {
    index: i64,
    sum: f64,
    count: u64,
}

impl Minute {
    fn mean(&self) -> Option<f64> {
        (self.count >= MIN_READINGS).then(|| self.sum / self.count as f64)
    }
}

/// UTC minute ring. NaN marks a slot without coverage. The pending minute
/// stays separate so it cannot overwrite the oldest completed minute early.
pub(super) struct MinuteHistory {
    completed: Box<[f64; RETENTION_MINUTES]>,
    pending: Option<Minute>,
}

impl Default for MinuteHistory {
    fn default() -> Self {
        Self {
            completed: Box::new([f64::NAN; RETENTION_MINUTES]),
            pending: None,
        }
    }
}

fn slot(minute: i64) -> usize {
    minute.rem_euclid(RETENTION_MINUTES as i64) as usize
}

impl MinuteHistory {
    pub(super) fn record(&mut self, utc_ms: i64, percent: f32) {
        let index = utc_ms.div_euclid(MINUTE_MS);
        if let Some(previous) = &self.pending {
            // After a clock correction, wait for UTC to catch up before
            // accepting samples from minutes that already passed.
            if index < previous.index {
                return;
            }
            if index > previous.index {
                if index - previous.index > RETENTION_MINUTES as i64 {
                    self.completed.fill(f64::NAN);
                } else {
                    self.completed[slot(previous.index)] = previous.mean().unwrap_or(f64::NAN);
                    for minute in previous.index + 1..index {
                        self.completed[slot(minute)] = f64::NAN;
                    }
                }
                self.pending = None;
            }
        }
        let pending = self.pending.get_or_insert(Minute {
            index,
            sum: 0.0,
            count: 0,
        });
        pending.sum += f64::from(percent);
        pending.count += 1;
    }

    fn minute_mean(&self, minute: i64, now_minute: i64) -> Option<f64> {
        if minute >= now_minute || minute < now_minute - RETENTION_MINUTES as i64 {
            return None;
        }
        let pending = self.pending.as_ref()?;
        if minute == pending.index {
            return pending.mean();
        }
        if minute > pending.index || minute < pending.index - RETENTION_MINUTES as i64 {
            return None;
        }
        let mean = self.completed[slot(minute)];
        mean.is_finite().then_some(mean)
    }

    fn bucket_mean(&self, start: i64, width: i64, now_minute: i64) -> Option<f64> {
        let mut sum = 0.0;
        let mut present = 0;
        for minute in start..start + width {
            if let Some(mean) = self.minute_mean(minute, now_minute) {
                sum += mean;
                present += 1;
            }
        }
        (present * 2 >= width).then(|| (sum * 10.0 / present as f64).round() / 10.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_minute(history: &mut MinuteHistory, minute: i64, count: u64, percent: f32) {
        for second in 0..count {
            history.record(minute * MINUTE_MS + second as i64 * 1_000, percent);
        }
    }

    #[test]
    fn ring_wraps_at_24_hours_and_drops_stale_slots_after_a_gap() {
        let mut history = MinuteHistory::default();
        let capacity = RETENTION_MINUTES as i64;
        for minute in 0..capacity {
            record_minute(&mut history, minute, 30, 20.0);
        }
        record_minute(&mut history, capacity, 30, 42.0);
        assert_eq!(history.completed.len(), 1440);
        assert_eq!(history.minute_mean(0, capacity), Some(20.0));
        assert_eq!(history.minute_mean(0, capacity + 1), None);
        assert_eq!(history.minute_mean(capacity, capacity + 1), Some(42.0));

        record_minute(&mut history, capacity + 4, 30, 55.0);
        assert_eq!(history.minute_mean(capacity + 1, capacity + 5), None);
        assert_eq!(history.minute_mean(capacity + 4, capacity + 5), Some(55.0));

        let after_gap = 3 * capacity;
        record_minute(&mut history, after_gap, 30, 15.0);
        assert_eq!(history.minute_mean(2 * capacity, after_gap + 1), None);
        assert_eq!(history.minute_mean(after_gap, after_gap + 1), Some(15.0));
    }

    #[test]
    fn minute_averages_require_30_readings_and_keep_gaps() {
        let mut history = MinuteHistory::default();
        record_minute(&mut history, 0, 29, 99.0);
        for sec in 0..30 {
            let percent = if sec < 10 { 12.0 } else { 42.0 };
            history.record(2 * MINUTE_MS + sec * 1_000, percent);
        }
        record_minute(&mut history, 3, 30, 0.0);

        assert_eq!(history.minute_mean(0, 4), None);
        assert_eq!(history.minute_mean(1, 4), None);
        assert_eq!(history.minute_mean(2, 4), Some(32.0));
        assert_eq!(history.minute_mean(3, 3), None);
        assert_eq!(history.minute_mean(3, 4), Some(0.0));
    }

    #[test]
    fn buckets_align_in_utc_and_apply_half_coverage_to_partial_buckets() -> Result<(), &'static str>
    {
        let mut history = MinuteHistory::default();
        record_minute(&mut history, 5, 30, 1.25);
        record_minute(&mut history, 6, 30, 2.25);
        record_minute(&mut history, 7, 30, 3.25);
        let range = Range::SixHours;
        let series_at = |minute| {
            range
                .series("atlas", minute * MINUTE_MS, &history)
                .map_err(|error| error.code)
        };

        // Two of five minutes fail coverage even though both are present
        // in the elapsed part of the last bucket.
        assert_eq!(series_at(7)?.samples[71], None);

        let series = series_at(8)?;
        assert_eq!(series.bucket_seconds, 300);
        assert_eq!(series.samples.len(), 72);
        assert!(series.samples[..71].iter().all(Option::is_none));
        assert_eq!(series.samples[71], Some(2.3));
        assert_eq!(series.to, "1970-01-01T00:10:00Z");
        assert_eq!(series_at(10)?.samples[71], Some(2.3));
        assert_eq!(series_at(11)?.samples[71], None);
        let hour = Range::Hour
            .series("atlas", 8 * MINUTE_MS + 30_000, &history)
            .map_err(|error| error.code)?;
        assert_eq!(hour.to, "1970-01-01T00:08:00Z");
        assert_eq!(hour.samples[59], Some(3.3));
        Ok(())
    }
}
