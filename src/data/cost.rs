//! Token prices in USD per million tokens, by model family. Cache reads and writes are billed
//! differently from fresh input, so the four buckets are kept apart.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Prices as of 2026-10 (Anthropic first-party API). Unknown models fall back to Opus 5.5.
pub fn price_for(model: &str) -> Price {
    let m = model.to_ascii_lowercase();
    let p = |input: f64, output: f64, cache_read: f64| Price {
        input,
        output,
        cache_read,
        cache_write: input * 1.25,
    };
    if m.contains("fable") || m.contains("mythos") {
        p(10.0, 50.0, 0.25)
    } else if m.contains("opus-5-5") {
        p(4.0, 20.0, 0.20)
    } else if m.contains("opus-5") || m.contains("opus-4") {
        p(5.0, 25.0, 0.50)
    } else if m.contains("sonnet-5") {
        p(2.0, 10.0, 0.20)
    } else if m.contains("sonnet-4") {
        p(3.0, 15.0, 0.30)
    } else if m.contains("haiku-5") {
        p(0.10, 0.50, 0.01)
    } else if m.contains("haiku-4") {
        p(1.0, 5.0, 0.10)
    } else {
        p(4.0, 20.0, 0.20)
    }
}

pub fn cost_usd(model: &str, input: u64, cache_read: u64, cache_write: u64, output: u64) -> f64 {
    let p = price_for(model);
    (input as f64 * p.input
        + cache_read as f64 * p.cache_read
        + cache_write as f64 * p.cache_write
        + output as f64 * p.output)
        / 1_000_000.0
}

pub fn fmt_usd(v: f64) -> String {
    if v < 0.01 {
        "$0.00".into()
    } else if v < 10.0 {
        format!("${v:.2}")
    } else {
        format!("${v:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_55_costs_what_the_table_says() {
        // 1M fresh input + 1M output on Opus 5.5 = $24.
        let c = cost_usd("claude-opus-5-5", 1_000_000, 0, 0, 1_000_000);
        assert!((c - 24.0).abs() < 1e-9);
        // Cache reads are a tenth of input price on Opus 5.5 ... (0.20 vs 4.00 is a twentieth).
        let r = cost_usd("claude-opus-5-5", 0, 1_000_000, 0, 0);
        assert!((r - 0.20).abs() < 1e-9);
        assert_eq!(fmt_usd(0.004), "$0.00");
        assert_eq!(fmt_usd(1.234), "$1.23");
        assert_eq!(fmt_usd(42.7), "$43");
    }
}
