//! Process resource-usage model for the status-bar CPU/RAM readout.
//!
//! Pure: the app boundary feeds raw `sysinfo` numbers into [`ResourceSample::new`]
//! (which normalizes CPU across cores) and the footer renders
//! [`ResourceSample::cpu_label`] / [`ResourceSample::ram_label`]. No sampling / I/O
//! lives here — that stays at the app boundary.

/// A single reading of the app process's own resource usage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResourceSample {
    /// Resident memory, bytes.
    pub mem_bytes: u64,
    /// CPU usage normalized to 0..=100 across all cores (Task-Manager style).
    pub cpu_pct: f32,
}

impl ResourceSample {
    /// Build a sample from raw `sysinfo` numbers.
    ///
    /// - `mem_bytes`: process resident memory.
    /// - `raw_cpu_pct`: `sysinfo`'s per-process CPU%, expressed relative to a single
    ///   core (so it can exceed 100 on multi-core machines).
    /// - `cores`: logical CPU count used to normalize CPU down to 0..=100.
    pub fn new(mem_bytes: u64, raw_cpu_pct: f32, cores: usize) -> Self {
        Self {
            mem_bytes,
            cpu_pct: normalize_cpu(raw_cpu_pct, cores),
        }
    }

    /// `"4%"` — normalized CPU rounded to a whole percent.
    pub fn cpu_label(&self) -> String {
        format!("{}%", self.cpu_pct.round() as u32)
    }

    /// `"143 MB"` — resident memory in MB.
    pub fn ram_label(&self) -> String {
        format!("{} MB", self.mem_bytes / (1024 * 1024))
    }
}

/// Normalize `sysinfo`'s single-core-relative CPU% to a whole-machine 0..=100
/// figure by dividing across `cores`, clamped. Zero cores yields 0.
fn normalize_cpu(raw_pct: f32, cores: usize) -> f32 {
    if cores == 0 {
        return 0.0;
    }
    (raw_pct / cores as f32).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_cpu_divides_across_cores() {
        assert_eq!(normalize_cpu(800.0, 8), 100.0);
        assert_eq!(normalize_cpu(400.0, 8), 50.0);
        assert_eq!(normalize_cpu(0.0, 8), 0.0);
    }

    #[test]
    fn normalize_cpu_zero_cores_is_zero() {
        assert_eq!(normalize_cpu(50.0, 0), 0.0);
    }

    #[test]
    fn normalize_cpu_clamps_to_hundred() {
        // A single busy thread on a single logical core shouldn't overshoot.
        assert_eq!(normalize_cpu(140.0, 1), 100.0);
    }

    #[test]
    fn cpu_label_rounds() {
        let s = ResourceSample {
            cpu_pct: 4.6,
            ..Default::default()
        };
        assert_eq!(s.cpu_label(), "5%");
    }

    #[test]
    fn ram_label_shows_mb_only() {
        let s = ResourceSample {
            mem_bytes: 143 * 1024 * 1024,
            cpu_pct: 0.0,
        };
        assert_eq!(s.ram_label(), "143 MB");
    }

    #[test]
    fn new_normalizes_cpu_and_keeps_bytes() {
        let s = ResourceSample::new(200 * 1024 * 1024, 200.0, 8);
        assert_eq!(s.mem_bytes, 200 * 1024 * 1024);
        assert_eq!(s.cpu_pct, 25.0);
        assert_eq!(s.cpu_label(), "25%");
        assert_eq!(s.ram_label(), "200 MB");
    }

    #[test]
    fn default_sample_is_zeroed() {
        let s = ResourceSample::default();
        assert_eq!(s.cpu_label(), "0%");
        assert_eq!(s.ram_label(), "0 MB");
    }
}
