//! Percentiles and other small statistics.

/// The `q` quantile (0 to 1) of `v`, by nearest rank on a sorted copy. `None` when empty.
pub fn quantile(v: &[f64], q: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return None;
    }
    s.sort_by(f64::total_cmp);
    let rank = ((q.clamp(0.0, 1.0) * s.len() as f64).ceil() as usize).clamp(1, s.len());
    Some(s[rank - 1])
}

pub fn median(v: &[f64]) -> Option<f64> {
    quantile(v, 0.5)
}

pub fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// Least-squares line `y = a + b x`: (a, b).
pub fn line(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let n = x.len().min(y.len());
    if n < 2 {
        return None;
    }
    let mx = x[..n].iter().sum::<f64>() / n as f64;
    let my = y[..n].iter().sum::<f64>() / n as f64;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for i in 0..n {
        sxy += (x[i] - mx) * (y[i] - my);
        sxx += (x[i] - mx) * (x[i] - mx);
    }
    if sxx <= 0.0 {
        return None;
    }
    let b = sxy / sxx;
    Some((my - b * mx, b))
}

/// Decibels of an amplitude ratio.
pub fn db(ratio: f64) -> f64 {
    20.0 * ratio.max(1e-30).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_by_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(quantile(&v, 0.95), Some(95.0));
        assert_eq!(quantile(&v, 1.0), Some(100.0));
        assert_eq!(median(&v), Some(50.0));
        assert_eq!(quantile(&[], 0.5), None);
    }

    #[test]
    fn a_line_through_points() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [1.0, 3.0, 5.0, 7.0];
        let (a, b) = line(&x, &y).unwrap();
        assert!((a - 1.0).abs() < 1e-12 && (b - 2.0).abs() < 1e-12);
    }
}
