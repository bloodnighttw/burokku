use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

/// Cheap, shared performance counters for diagnosing DOM and frame stalls.
///
/// Recording uses relaxed atomics only. The values are diagnostic rather than
/// synchronization primitives, so metrics never add a lock to the DOM or frame
/// paths.
#[derive(Clone, Debug, Default)]
pub struct PerformanceMetrics {
    inner: Arc<PerformanceMetricsInner>,
}

#[derive(Debug, Default)]
struct PerformanceMetricsInner {
    commits: AtomicU64,
    latest_snapshot_creation_ns: AtomicU64,
    max_snapshot_creation_ns: AtomicU64,
    latest_publication_ns: AtomicU64,
    max_publication_ns: AtomicU64,
    frame_attempts: AtomicU64,
    frames_presented: AtomicU64,
    latest_frame_ns: AtomicU64,
    max_frame_ns: AtomicU64,
    latest_layout_ns: AtomicU64,
    max_layout_ns: AtomicU64,
    latest_scene_ns: AtomicU64,
    max_scene_ns: AtomicU64,
    latest_vello_ns: AtomicU64,
    max_vello_ns: AtomicU64,
    latest_commit_to_present_ns: AtomicU64,
    max_commit_to_present_ns: AtomicU64,
    coalesced_redraw_requests: AtomicU64,
    coalesced_revisions: AtomicU64,
    dropped_events: AtomicU64,
    bts_queue_high_water: AtomicU64,
}

/// One internally consistent-enough diagnostic reading of the relaxed metrics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PerformanceMetricsSnapshot {
    pub commits: u64,
    pub latest_snapshot_creation: Duration,
    pub max_snapshot_creation: Duration,
    pub latest_publication: Duration,
    pub max_publication: Duration,
    pub frame_attempts: u64,
    pub frames_presented: u64,
    pub latest_frame: Duration,
    pub max_frame: Duration,
    pub latest_layout: Duration,
    pub max_layout: Duration,
    pub latest_scene_construction: Duration,
    pub max_scene_construction: Duration,
    pub latest_vello_render: Duration,
    pub max_vello_render: Duration,
    pub latest_commit_to_present: Duration,
    pub max_commit_to_present: Duration,
    pub coalesced_redraw_requests: u64,
    pub coalesced_revisions: u64,
    pub dropped_events: u64,
    pub bts_queue_high_water: usize,
}

impl PerformanceMetrics {
    pub fn snapshot(&self) -> PerformanceMetricsSnapshot {
        let inner = &self.inner;
        PerformanceMetricsSnapshot {
            commits: load(&inner.commits),
            latest_snapshot_creation: duration(load(&inner.latest_snapshot_creation_ns)),
            max_snapshot_creation: duration(load(&inner.max_snapshot_creation_ns)),
            latest_publication: duration(load(&inner.latest_publication_ns)),
            max_publication: duration(load(&inner.max_publication_ns)),
            frame_attempts: load(&inner.frame_attempts),
            frames_presented: load(&inner.frames_presented),
            latest_frame: duration(load(&inner.latest_frame_ns)),
            max_frame: duration(load(&inner.max_frame_ns)),
            latest_layout: duration(load(&inner.latest_layout_ns)),
            max_layout: duration(load(&inner.max_layout_ns)),
            latest_scene_construction: duration(load(&inner.latest_scene_ns)),
            max_scene_construction: duration(load(&inner.max_scene_ns)),
            latest_vello_render: duration(load(&inner.latest_vello_ns)),
            max_vello_render: duration(load(&inner.max_vello_ns)),
            latest_commit_to_present: duration(load(&inner.latest_commit_to_present_ns)),
            max_commit_to_present: duration(load(&inner.max_commit_to_present_ns)),
            coalesced_redraw_requests: load(&inner.coalesced_redraw_requests),
            coalesced_revisions: load(&inner.coalesced_revisions),
            dropped_events: load(&inner.dropped_events),
            bts_queue_high_water: usize::try_from(load(&inner.bts_queue_high_water))
                .unwrap_or(usize::MAX),
        }
    }

    pub(crate) fn record_commit(&self, snapshot_creation: Duration, publication: Duration) {
        self.inner.commits.fetch_add(1, Ordering::Relaxed);
        record_duration(
            &self.inner.latest_snapshot_creation_ns,
            &self.inner.max_snapshot_creation_ns,
            snapshot_creation,
        );
        record_duration(
            &self.inner.latest_publication_ns,
            &self.inner.max_publication_ns,
            publication,
        );
    }

    pub(crate) fn record_frame_attempt(&self) {
        self.inner.frame_attempts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_presented_frame(
        &self,
        total: Duration,
        layout: Duration,
        scene: Duration,
        vello: Duration,
        commit_to_present: Duration,
        coalesced_revisions: u64,
    ) {
        self.inner.frames_presented.fetch_add(1, Ordering::Relaxed);
        record_duration(&self.inner.latest_frame_ns, &self.inner.max_frame_ns, total);
        record_duration(
            &self.inner.latest_layout_ns,
            &self.inner.max_layout_ns,
            layout,
        );
        record_duration(&self.inner.latest_scene_ns, &self.inner.max_scene_ns, scene);
        record_duration(&self.inner.latest_vello_ns, &self.inner.max_vello_ns, vello);
        record_duration(
            &self.inner.latest_commit_to_present_ns,
            &self.inner.max_commit_to_present_ns,
            commit_to_present,
        );
        self.inner
            .coalesced_revisions
            .fetch_add(coalesced_revisions, Ordering::Relaxed);
    }

    pub(crate) fn record_coalesced_redraw(&self) {
        self.inner
            .coalesced_redraw_requests
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_dropped_event(&self) {
        self.inner.dropped_events.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn observe_bts_queue_depth(&self, depth: usize) {
        update_max(
            &self.inner.bts_queue_high_water,
            u64::try_from(depth).unwrap_or(u64::MAX),
        );
    }
}

fn load(value: &AtomicU64) -> u64 {
    value.load(Ordering::Relaxed)
}

fn duration(nanoseconds: u64) -> Duration {
    Duration::from_nanos(nanoseconds)
}

fn record_duration(latest: &AtomicU64, maximum: &AtomicU64, value: Duration) {
    let value = u64::try_from(value.as_nanos()).unwrap_or(u64::MAX);
    latest.store(value, Ordering::Relaxed);
    update_max(maximum, value);
}

fn update_max(maximum: &AtomicU64, value: u64) {
    maximum.fetch_max(value, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_accumulate_counts_and_duration_high_water_marks() {
        let metrics = PerformanceMetrics::default();
        metrics.record_commit(Duration::from_nanos(10), Duration::from_nanos(20));
        metrics.record_commit(Duration::from_nanos(5), Duration::from_nanos(30));
        metrics.record_frame_attempt();
        metrics.record_presented_frame(
            Duration::from_nanos(100),
            Duration::from_nanos(40),
            Duration::from_nanos(20),
            Duration::from_nanos(30),
            Duration::from_nanos(200),
            3,
        );
        metrics.record_coalesced_redraw();
        metrics.record_dropped_event();
        metrics.observe_bts_queue_depth(4);
        metrics.observe_bts_queue_depth(2);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.commits, 2);
        assert_eq!(snapshot.latest_snapshot_creation, Duration::from_nanos(5));
        assert_eq!(snapshot.max_snapshot_creation, Duration::from_nanos(10));
        assert_eq!(snapshot.latest_publication, Duration::from_nanos(30));
        assert_eq!(snapshot.max_publication, Duration::from_nanos(30));
        assert_eq!(snapshot.frame_attempts, 1);
        assert_eq!(snapshot.frames_presented, 1);
        assert_eq!(snapshot.coalesced_revisions, 3);
        assert_eq!(snapshot.coalesced_redraw_requests, 1);
        assert_eq!(snapshot.dropped_events, 1);
        assert_eq!(snapshot.bts_queue_high_water, 4);
    }
}
