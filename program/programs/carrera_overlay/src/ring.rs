//! 24-sample ring buffer of hourly funding rates.
//!
//! Samples are the hourly funding rate in bps × 1e6 (`bps_e6`), so a 35%
//! annualised rate is an hourly sample of 0.3995 bps = 399_543. The average is
//! annualised by multiplying by 8760 hours and converting back to plain bps.

pub const HOURS_PER_YEAR: i128 = 8760;
pub const E6: i128 = 1_000_000;

pub fn push(buf: &mut [i64], head: &mut u8, samples: &mut u8, value: i64) {
    let n = buf.len();
    buf[*head as usize] = value;
    *head = ((*head as usize + 1) % n) as u8;
    if (*samples as usize) < n {
        *samples += 1;
    }
}

/// Mean of the recorded samples × 8760, in annualised bps. `None` when empty.
pub fn f_avg_bps(buf: &[i64], samples: u8) -> Option<i64> {
    let n = (samples as usize).min(buf.len());
    if n == 0 {
        return None;
    }
    let sum: i128 = buf[..n].iter().map(|v| *v as i128).sum();
    // The first `n` slots are always the populated ones (head wraps only after buf is full).
    let avg = sum
        .checked_mul(HOURS_PER_YEAR)?
        .checked_div(n as i128 * E6)?;
    i64::try_from(avg).ok()
}

/// Hourly sample (bps_e6) that corresponds to an annualised rate in bps.
pub fn hourly_sample_from_annual_bps(annual_bps: i64) -> i64 {
    ((annual_bps as i128) * E6 / HOURS_PER_YEAR) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_has_no_average() {
        let buf = [0i64; 24];
        assert_eq!(f_avg_bps(&buf, 0), None);
    }

    #[test]
    fn single_sample_annualises() {
        let mut buf = [0i64; 24];
        let (mut head, mut samples) = (0u8, 0u8);
        push(&mut buf, &mut head, &mut samples, hourly_sample_from_annual_bps(3500));
        let avg = f_avg_bps(&buf, samples).unwrap();
        assert!((avg - 3500).abs() <= 1, "got {avg}");
    }

    #[test]
    fn wraps_and_saturates_at_24() {
        let mut buf = [0i64; 24];
        let (mut head, mut samples) = (0u8, 0u8);
        for _ in 0..24 {
            push(&mut buf, &mut head, &mut samples, hourly_sample_from_annual_bps(1000));
        }
        assert_eq!(samples, 24);
        assert_eq!(head, 0);
        // Overwrite everything with a higher rate; average follows.
        for _ in 0..24 {
            push(&mut buf, &mut head, &mut samples, hourly_sample_from_annual_bps(4000));
        }
        assert_eq!(samples, 24);
        let avg = f_avg_bps(&buf, samples).unwrap();
        assert!((avg - 4000).abs() <= 1, "got {avg}");
    }

    #[test]
    fn partial_window_averages_only_recorded_samples() {
        let mut buf = [0i64; 24];
        let (mut head, mut samples) = (0u8, 0u8);
        push(&mut buf, &mut head, &mut samples, hourly_sample_from_annual_bps(2000));
        push(&mut buf, &mut head, &mut samples, hourly_sample_from_annual_bps(4000));
        let avg = f_avg_bps(&buf, samples).unwrap();
        assert!((avg - 3000).abs() <= 1, "got {avg}");
    }

    #[test]
    fn negative_funding_is_allowed() {
        let mut buf = [0i64; 24];
        let (mut head, mut samples) = (0u8, 0u8);
        push(&mut buf, &mut head, &mut samples, hourly_sample_from_annual_bps(-1500));
        let avg = f_avg_bps(&buf, samples).unwrap();
        assert!((avg + 1500).abs() <= 1, "got {avg}");
    }
}
