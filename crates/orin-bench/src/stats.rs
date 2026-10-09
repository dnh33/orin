//! Statistics helpers: percentiles, throughput, memory.

use std::time::Duration;

pub struct Stats {
    pub latency_p50_us: f64,
    pub latency_p95_us: f64,
    pub latency_p99_us: f64,
    pub qps: f64,
    pub rss_peak_mb: f64,
    pub rss_steady_mb: f64,
}

pub fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub fn from_durations(latencies: &[Duration]) -> Stats {
    let us: Vec<f64> = latencies.iter().map(|d| d.as_micros() as f64).collect();

    let p50 = percentile(&us, 50.0);
    let p95 = percentile(&us, 95.0);
    let p99 = percentile(&us, 99.0);

    let total_us: f64 = us.iter().sum();
    let qps = if total_us > 0.0 {
        (latencies.len() as f64 / total_us) * 1_000_000.0
    } else {
        0.0
    };

    Stats {
        latency_p50_us: p50,
        latency_p95_us: p95,
        latency_p99_us: p99,
        qps,
        rss_peak_mb: 0.0,
        rss_steady_mb: 0.0,
    }
}

pub fn from_us(p50: f64, p95: f64, p99: f64, qps: f64) -> Stats {
    Stats {
        latency_p50_us: p50,
        latency_p95_us: p95,
        latency_p99_us: p99,
        qps,
        rss_peak_mb: 0.0,
        rss_steady_mb: 0.0,
    }
}
