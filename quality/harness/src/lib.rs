//! Non-production smoke crate for the M0 quality pipeline.
//!
//! This crate intentionally contains no Intention Relay product logic. It keeps
//! the virtual workspace executable so formatting, linting, tests, docs, and
//! coverage gates are proven before M1 creates product crates.

/// Returns the stable marker used by M0 quality tests.
#[must_use]
pub const fn quality_marker() -> &'static str {
    "m0"
}

#[cfg(test)]
mod tests {
    use super::quality_marker;

    #[test]
    fn returns_m0_marker() {
        assert_eq!(quality_marker(), "m0");
    }
}
