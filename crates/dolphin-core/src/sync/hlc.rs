//! Hybrid logical clocks. A timestamp is `<12 hex digits of ms>.<4 hex counter>.<node>`, so plain string
//! comparison is the clock order. Unlike wall-clock `updated_at`, a device whose clock runs slow still
//! orders its edits after anything it has already seen from other devices.

/// An operation stamped further ahead than this is treated as a broken clock and refused.
pub const MAX_DRIFT_MS: i64 = 24 * 3_600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Stamp {
    pub phys: i64,
    pub counter: u32,
}

pub fn encode(stamp: Stamp, node: &str) -> String {
    format!("{:012x}.{:04x}.{}", stamp.phys.max(0), stamp.counter, node)
}

pub fn parse(hlc: &str) -> Option<Stamp> {
    let mut parts = hlc.splitn(3, '.');
    let phys = i64::from_str_radix(parts.next()?, 16).ok()?;
    let counter = u32::from_str_radix(parts.next()?, 16).ok()?;
    parts.next()?;
    Some(Stamp { phys, counter })
}

/// The next stamp after `last`, for a local event at `at_ms` (the wall clock, or the time of an edit
/// that is only being captured now). Never goes backwards.
pub fn next(last: Stamp, at_ms: i64) -> Stamp {
    if at_ms > last.phys { Stamp { phys: at_ms, counter: 0 } } else { Stamp { phys: last.phys, counter: last.counter + 1 } }
}

/// The node suffix: the random tail of the device id, so two devices never tie.
pub fn node_of(device_id: &str) -> String {
    let compact: String = device_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    compact.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_order_is_clock_order() {
        let a = encode(Stamp { phys: 1_000, counter: 0 }, "aaaa");
        let b = encode(Stamp { phys: 1_000, counter: 1 }, "aaaa");
        let c = encode(Stamp { phys: 1_001, counter: 0 }, "aaaa");
        let d = encode(Stamp { phys: 1_001, counter: 0 }, "bbbb");
        assert!(a < b && b < c && c < d);
        assert_eq!(parse(&c), Some(Stamp { phys: 1_001, counter: 0 }));
        assert_eq!(parse("garbage"), None);
    }

    #[test]
    fn a_slow_clock_still_moves_forward() {
        let seen = Stamp { phys: 5_000, counter: 3 };
        assert_eq!(next(seen, 1_000), Stamp { phys: 5_000, counter: 4 });
        assert_eq!(next(seen, 9_000), Stamp { phys: 9_000, counter: 0 });
    }

    #[test]
    fn node_is_the_random_tail() {
        assert_eq!(node_of("019f2b3c-4d5e-7000-8000-0123456789ab"), "456789ab");
    }
}
