// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

/// Producer-local accounting for bounded latest-buffer acquisition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BufferLatestStats {
    /// Output acquisition duty cycles.
    pub dequeue_attempts: u64,
    /// Completed consumer leases examined.
    pub completions: u64,
    /// Reusable pool slots examined.
    pub buffer_probes: u64,
    /// Attempts that found no safely reusable allocation.
    pub pool_exhaustions: u64,
    /// Unclaimed submissions reclaimed after a full scan.
    pub submission_reclaims: u64,
    /// Subscriber submissions withdrawn while reclaiming a pool buffer.
    pub submission_withdrawals: u64,
    /// Output buffers offered to the active fan-out set.
    pub publications: u64,
    /// Active subscriber channels visited by publication.
    pub subscriber_visits: u64,
    /// Subscriber-local leases created by publication.
    pub subscriber_deliveries: u64,
    /// Subscriber-local unclaimed submissions replaced by a newer publication.
    pub submission_overflows: u64,
    /// Retired subscriber slots acknowledged by the producer.
    pub subscriber_retirements: u64,
    /// Outstanding subscriber leases recovered during retirement.
    pub retired_leases: u64,
    /// Publications that raced with removal and reached no active subscriber.
    pub zero_recipient_publications: u64,
    /// Largest number of pool slots examined by one scan.
    pub max_buffer_probes: u32,
    /// Largest aggregate completion drain in one acquisition attempt.
    pub max_completions: u32,
    /// Largest number of submissions withdrawn in one reclaim attempt.
    pub max_submission_withdrawals: u32,
    /// Largest active fan-out visited by one publication.
    pub max_subscriber_visits: u32,
}

impl From<pw_sys::pw_buffer_latest_stats> for BufferLatestStats {
    fn from(raw: pw_sys::pw_buffer_latest_stats) -> Self {
        Self {
            dequeue_attempts: raw.dequeue_attempts,
            completions: raw.completions,
            buffer_probes: raw.buffer_probes,
            pool_exhaustions: raw.pool_exhaustions,
            submission_reclaims: raw.submission_reclaims,
            submission_withdrawals: raw.submission_withdrawals,
            publications: raw.publications,
            subscriber_visits: raw.subscriber_visits,
            subscriber_deliveries: raw.subscriber_deliveries,
            submission_overflows: raw.submission_overflows,
            subscriber_retirements: raw.subscriber_retirements,
            retired_leases: raw.retired_leases,
            zero_recipient_publications: raw.zero_recipient_publications,
            max_buffer_probes: raw.max_buffer_probes,
            max_completions: raw.max_completions,
            max_submission_withdrawals: raw.max_submission_withdrawals,
            max_subscriber_visits: raw.max_subscriber_visits,
        }
    }
}
