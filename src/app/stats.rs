use crate::register::RegisterType;
use crate::state::StatsTab;
use chrono::{DateTime, Utc};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatTarget {
    Read(RegisterType, u16, u16),
    Write(RegisterType, u16, u16),
    Function(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatError {
    pub message: String,
    pub at: DateTime<Utc>,
    pub target: Option<StatTarget>,
}

#[derive(Debug, Default, Clone)]
pub struct OpStats {
    pub ok: u64,
    pub errors: u64,
    latency_sum: Duration,
    latency_min: Option<Duration>,
    latency_max: Option<Duration>,
    last_error: Option<StatError>,
}

impl OpStats {
    pub fn record_ok(&mut self, latency: Duration) {
        self.ok += 1;
        self.latency_sum += latency;
        self.latency_min = Some(self.latency_min.map_or(latency, |m| m.min(latency)));
        self.latency_max = Some(self.latency_max.map_or(latency, |m| m.max(latency)));
    }

    pub fn record_error(&mut self, message: &str, target: Option<StatTarget>) {
        self.errors += 1;
        self.last_error = Some(StatError {
            message: message.to_string(),
            at: Utc::now(),
            target,
        });
    }

    pub fn latency(&self) -> Option<(Duration, Duration, Duration)> {
        let (min, max) = (self.latency_min?, self.latency_max?);
        let avg = self.latency_sum / self.ok.min(u32::MAX as u64) as u32;
        Some((min, avg, max))
    }

    pub fn error_rate(&self) -> Option<f64> {
        (self.errors > 0).then(|| self.errors as f64 * 100.0 / (self.ok + self.errors) as f64)
    }

    pub const fn last_error(&self) -> Option<&StatError> {
        self.last_error.as_ref()
    }

    fn merge(mut self, other: &Self) -> Self {
        self.ok += other.ok;
        self.errors += other.errors;
        self.latency_sum += other.latency_sum;
        self.latency_min = match (self.latency_min, other.latency_min) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.latency_max = self.latency_max.max(other.latency_max);
        if let Some(theirs) = &other.last_error
            && self
                .last_error
                .as_ref()
                .is_none_or(|ours| theirs.at >= ours.at)
        {
            self.last_error = Some(theirs.clone());
        }
        self
    }
}

#[derive(Debug, Default)]
pub struct CommStats {
    pub reads: OpStats,
    pub writes: OpStats,
    pub others: OpStats,
}

impl CommStats {
    pub fn tab(&self, tab: StatsTab) -> OpStats {
        match tab {
            StatsTab::All => self.reads.clone().merge(&self.writes).merge(&self.others),
            StatsTab::Reads => self.reads.clone(),
            StatsTab::Writes => self.writes.clone(),
            StatsTab::Others => self.others.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_all_tab_sums_counts_and_spans_latencies() {
        let mut stats = CommStats::default();
        stats.reads.record_ok(Duration::from_millis(4));
        stats.reads.record_ok(Duration::from_millis(6));
        stats.writes.record_ok(Duration::from_millis(20));
        stats
            .others
            .record_error("Illegal function", Some(StatTarget::Function(0x41)));

        let all = stats.tab(StatsTab::All);
        assert_eq!((all.ok, all.errors), (3, 1));
        assert_eq!(
            all.latency(),
            Some((
                Duration::from_millis(4),
                Duration::from_millis(10),
                Duration::from_millis(20)
            ))
        );
        assert_eq!(all.error_rate(), Some(25.0));
    }

    #[test]
    fn the_all_tab_shows_the_newest_error_of_any_kind() {
        let mut stats = CommStats::default();
        stats.reads.record_error(
            "read timed out",
            Some(StatTarget::Read(RegisterType::Input, 0, 10)),
        );
        stats.writes.record_error(
            "Illegal data address",
            Some(StatTarget::Write(RegisterType::Holding, 16, 1)),
        );

        let all = stats.tab(StatsTab::All);
        let error = all.last_error().expect("an error was recorded");
        assert_eq!(error.message, "Illegal data address");
        assert_eq!(
            error.target,
            Some(StatTarget::Write(RegisterType::Holding, 16, 1))
        );
        assert_eq!(stats.tab(StatsTab::Others).last_error(), None);
    }

    #[test]
    fn no_successes_means_no_latency_and_no_errors_means_no_rate() {
        let mut stats = OpStats::default();
        assert_eq!(stats.latency(), None);
        assert_eq!(stats.error_rate(), None);
        stats.record_error("boom", None);
        assert_eq!(stats.latency(), None);
        assert_eq!(stats.error_rate(), Some(100.0));
    }
}
