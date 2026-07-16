use std::time::Duration;

use anyhow::{bail, Result};

pub fn parse_interval(input: &str) -> Result<Duration> {
    let s = input.trim();
    if s.is_empty() {
        bail!("empty interval");
    }

    if s == "0" {
        return Ok(Duration::ZERO);
    }

    if let Ok(secs) = s.parse::<u64>() {
        return Ok(Duration::from_secs(secs));
    }

    let (num_part, unit) = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| s.split_at(i))
        .unwrap_or((s, ""));

    let value: u64 = num_part.parse().map_err(|_| anyhow::anyhow!("invalid interval: {s}"))?;
    let secs = match unit {
        "" | "s" | "sec" | "secs" => value,
        "m" | "min" | "mins" => value * 60,
        "h" | "hr" | "hrs" => value * 3600,
        _ => bail!("unknown interval unit in {s}"),
    };

    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_zero_disables() {
        assert_eq!(parse_interval("0").unwrap(), Duration::ZERO);
    }

    #[test]
    fn parse_seconds() {
        assert_eq!(parse_interval("30").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_interval("45s").unwrap(), Duration::from_secs(45));
        assert_eq!(parse_interval("5m").unwrap(), Duration::from_secs(300));
    }
}
