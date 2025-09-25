use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Stats {
    pub start_time: Instant,
    pub total_contracts: AtomicUsize,
    pub processed: AtomicUsize,
    pub cached: AtomicUsize,
    pub successes: AtomicUsize,
    pub errors: AtomicUsize,
    pub timeouts: AtomicUsize,
    pub total_processing_time: AtomicU64, // in microseconds
}

impl Stats {
    pub fn new() -> Arc<Self> {
        Arc::new(Stats {
            start_time: Instant::now(),
            total_contracts: AtomicUsize::new(0),
            processed: AtomicUsize::new(0),
            cached: AtomicUsize::new(0),
            successes: AtomicUsize::new(0),
            errors: AtomicUsize::new(0),
            timeouts: AtomicUsize::new(0),
            total_processing_time: AtomicU64::new(0),
        })
    }

    pub fn record_result(
        &self,
        cached: bool,
        success: bool,
        is_timeout: bool,
        duration: Duration,
    ) {
        self.processed.fetch_add(1, Ordering::Relaxed);

        if cached {
            self.cached.fetch_add(1, Ordering::Relaxed);
        } else if success {
            self.successes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.errors.fetch_add(1, Ordering::Relaxed);
            if is_timeout {
                self.timeouts.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Add processing time in microseconds
        self.total_processing_time
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    pub fn get_summary(&self) -> String {
        let elapsed = self.start_time.elapsed();
        let processed = self.processed.load(Ordering::Relaxed);
        let cached = self.cached.load(Ordering::Relaxed);
        let successes = self.successes.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let timeouts = self.timeouts.load(Ordering::Relaxed);
        let total = self.total_contracts.load(Ordering::Relaxed);

        let throughput = if elapsed.as_secs() > 0 {
            processed as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        let avg_time = if processed > 0 {
            let total_micros = self.total_processing_time.load(Ordering::Relaxed);
            Duration::from_micros(total_micros / processed as u64)
        } else {
            Duration::ZERO
        };

        let progress_pct = if total > 0 {
            (processed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let success_rate = if processed > cached {
            let actual_processed = processed - cached;
            if actual_processed > 0 {
                (successes as f64 / actual_processed as f64) * 100.0
            } else {
                100.0
            }
        } else {
            100.0
        };

        format!(
            "Progress: {}/{} ({:.1}%) | Cached: {} | Success: {} | Errors: {} (Timeouts: {}) | Rate: {:.1}/s | Avg: {:.2}ms | Success Rate: {:.1}%",
            processed,
            total,
            progress_pct,
            cached,
            successes,
            errors,
            timeouts,
            throughput,
            avg_time.as_millis(),
            success_rate
        )
    }

    pub fn get_final_summary(&self) -> String {
        let elapsed = self.start_time.elapsed();
        let processed = self.processed.load(Ordering::Relaxed);
        let cached = self.cached.load(Ordering::Relaxed);
        let successes = self.successes.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let timeouts = self.timeouts.load(Ordering::Relaxed);
        let total = self.total_contracts.load(Ordering::Relaxed);

        let throughput = if elapsed.as_secs() > 0 {
            processed as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        let success_rate = if processed > cached {
            let actual_processed = processed - cached;
            if actual_processed > 0 {
                (successes as f64 / actual_processed as f64) * 100.0
            } else {
                100.0
            }
        } else {
            100.0
        };

        format!(
            r#"
=== Final Summary ===
Total contracts:     {}
Processed:          {}
  - Cached:         {}
  - New successes:  {}
  - Errors:         {}
    - Timeouts:     {}
    - Other:        {}
Success rate:       {:.1}%
Total time:         {:.2}s
Overall throughput: {:.1} contracts/sec
"#,
            total,
            processed,
            cached,
            successes,
            errors,
            timeouts,
            errors - timeouts,
            success_rate,
            elapsed.as_secs_f64(),
            throughput
        )
    }
}