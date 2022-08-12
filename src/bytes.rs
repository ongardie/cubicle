//! Displays numbers of bytes with SI units.
//!
//! See the unit tests for examples.
//!
//! Similar crates include `bytesize` and `humansize`. Both, however, have
//! concerning open issues, potentially causing maintenance headaches in the
//! future. This implementation, on the other hand, is self-contained, fairly
//! simple, and easily tested.

use std::fmt;

/// A count of bytes. This type is useful for its [`fmt::Display`] impl.
pub struct Bytes(pub u64);

impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 1_000 {
            write!(f, "{} B", self.0)
        } else if self.0 < 999_950 {
            write!(f, "{:.1} kB", (self.0 as f64) / 1e3)
        } else if self.0 < 999_950_000 {
            write!(f, "{:.1} MB", (self.0 as f64) / 1e6)
        } else if self.0 < 999_950_000_000 {
            write!(f, "{:.1} GB", (self.0 as f64) / 1e9)
        } else if self.0 < 999_950_000_000_000 {
            write!(f, "{:.1} TB", (self.0 as f64) / 1e12)
        } else if self.0 < 999_950_000_000_000_000 {
            write!(
                f,
                "{:.1} PB",
                if self.0 <= 9_007_199_254_740_992 {
                    (self.0 as f64) / 1e15
                } else {
                    // Larger integers can't be represented exactly in an f64.
                    // It's probably best to divide some first.
                    (self.0 / 1_000_000_000_000) as f64 / 1e3
                }
            )
        } else {
            write!(f, "{:.1} EB", (self.0 / 1_000_000_000_000_000) as f64 / 1e3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        assert_eq!("0 B", Bytes(0).to_string());
        assert_eq!("999 B", Bytes(999).to_string());
        assert_eq!("1.0 kB", Bytes(1_000).to_string());
        assert_eq!("1.0 kB", Bytes(1_049).to_string());
        assert_eq!("1.1 kB", Bytes(1_050).to_string());
        assert_eq!("999.9 kB", Bytes(999_949).to_string());
        assert_eq!("1.0 MB", Bytes(999_950).to_string());
        assert_eq!("1.0 MB", Bytes(1_000_000).to_string());
        assert_eq!("999.9 MB", Bytes(999_949_999).to_string());
        assert_eq!("1.0 GB", Bytes(999_950_000).to_string());
        assert_eq!("999.9 GB", Bytes(999_949_999_999).to_string());
        assert_eq!("1.0 TB", Bytes(999_950_000_000).to_string());
        assert_eq!("999.9 TB", Bytes(999_949_999_999_999).to_string());
        assert_eq!("1.0 PB", Bytes(999_950_000_000_000).to_string());
        assert_eq!("9.0 PB", Bytes(9_007_199_254_740_992).to_string());
        assert_eq!("9.0 PB", Bytes(9_007_199_254_740_993).to_string());
        assert_eq!("999.9 PB", Bytes(999_949_999_999_999_999).to_string());
        assert_eq!("1.0 EB", Bytes(999_950_000_000_000_000).to_string());
        assert_eq!("18.4 EB", Bytes(u64::MAX).to_string());
    }
}
