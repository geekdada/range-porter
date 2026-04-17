use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct AggregateTotals {
    pub tcp_accepted: u64,
    pub tcp_active: u64,
    pub tcp_closed: u64,
    pub udp_active_flows: u64,
    pub tcp_in_bytes: u64,
    pub tcp_out_bytes: u64,
    pub udp_in_bytes: u64,
    pub udp_out_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MinuteBucket {
    pub minute_epoch: u64,
    pub tcp_new_conn: u64,
    pub tcp_active_peak: u64,
    pub udp_active_flows_peak: u64,
    pub tcp_in_bytes: u64,
    pub tcp_out_bytes: u64,
    pub udp_in_bytes: u64,
    pub udp_out_bytes: u64,
}

#[derive(Debug)]
pub struct BucketRing {
    buckets: Vec<MinuteBucket>,
}

impl BucketRing {
    pub fn new(window_minutes: usize) -> Self {
        Self {
            buckets: vec![MinuteBucket::default(); window_minutes.max(1)],
        }
    }

    pub fn rollup(
        &mut self,
        now_epoch_seconds: u64,
        totals: &AggregateTotals,
        previous_totals: &mut AggregateTotals,
    ) {
        let minute_epoch = now_epoch_seconds / 60;
        let index = (minute_epoch as usize) % self.buckets.len();
        let bucket = &mut self.buckets[index];

        if bucket.minute_epoch != minute_epoch {
            *bucket = MinuteBucket {
                minute_epoch,
                ..MinuteBucket::default()
            };
        }

        bucket.tcp_new_conn += totals
            .tcp_accepted
            .saturating_sub(previous_totals.tcp_accepted);
        bucket.tcp_active_peak = bucket.tcp_active_peak.max(totals.tcp_active);
        bucket.udp_active_flows_peak = bucket.udp_active_flows_peak.max(totals.udp_active_flows);
        bucket.tcp_in_bytes += totals
            .tcp_in_bytes
            .saturating_sub(previous_totals.tcp_in_bytes);
        bucket.tcp_out_bytes += totals
            .tcp_out_bytes
            .saturating_sub(previous_totals.tcp_out_bytes);
        bucket.udp_in_bytes += totals
            .udp_in_bytes
            .saturating_sub(previous_totals.udp_in_bytes);
        bucket.udp_out_bytes += totals
            .udp_out_bytes
            .saturating_sub(previous_totals.udp_out_bytes);

        *previous_totals = totals.clone();
    }

    pub fn snapshot(&self) -> Vec<MinuteBucket> {
        let mut buckets: Vec<_> = self
            .buckets
            .iter()
            .filter(|bucket| bucket.minute_epoch != 0)
            .cloned()
            .collect();
        buckets.sort_by_key(|bucket| bucket.minute_epoch);
        buckets
    }
}
