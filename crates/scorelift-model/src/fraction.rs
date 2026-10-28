use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Mul, Sub};

use serde::{Deserialize, Serialize};

/// A non-negative rational number, always stored in lowest terms.
///
/// Durations and offsets are measured in whole notes, so a quarter note is
/// `1/4` and an eighth-note triplet is `1/12`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "(u64, u64)", from = "(u64, u64)")]
pub struct Fraction {
    num: u64,
    den: u64,
}

impl Fraction {
    pub const ZERO: Fraction = Fraction { num: 0, den: 1 };

    /// Creates `num / den`, reduced. Panics if `den` is zero.
    pub fn new(num: u64, den: u64) -> Self {
        assert!(den != 0, "fraction denominator must not be zero");
        let g = gcd(num, den);
        Fraction {
            num: num / g,
            den: den / g,
        }
    }

    pub fn num(self) -> u64 {
        self.num
    }

    pub fn den(self) -> u64 {
        self.den
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }
}

impl Default for Fraction {
    fn default() -> Self {
        Fraction::ZERO
    }
}

impl From<(u64, u64)> for Fraction {
    fn from((num, den): (u64, u64)) -> Self {
        Fraction::new(num, den)
    }
}

impl From<Fraction> for (u64, u64) {
    fn from(f: Fraction) -> Self {
        (f.num, f.den)
    }
}

impl Add for Fraction {
    type Output = Fraction;
    fn add(self, rhs: Fraction) -> Fraction {
        let den = lcm(self.den, rhs.den);
        Fraction::new(self.num * (den / self.den) + rhs.num * (den / rhs.den), den)
    }
}

impl AddAssign for Fraction {
    fn add_assign(&mut self, rhs: Fraction) {
        *self = *self + rhs;
    }
}

impl Sub for Fraction {
    type Output = Fraction;
    /// Panics if the result would be negative.
    fn sub(self, rhs: Fraction) -> Fraction {
        let den = lcm(self.den, rhs.den);
        let a = self.num * (den / self.den);
        let b = rhs.num * (den / rhs.den);
        assert!(a >= b, "fraction subtraction underflow: {self} - {rhs}");
        Fraction::new(a - b, den)
    }
}

impl Mul for Fraction {
    type Output = Fraction;
    fn mul(self, rhs: Fraction) -> Fraction {
        // Cross-reduce first to keep the numbers small.
        let g1 = gcd(self.num, rhs.den);
        let g2 = gcd(rhs.num, self.den);
        Fraction::new(
            (self.num / g1) * (rhs.num / g2),
            (self.den / g2) * (rhs.den / g1),
        )
    }
}

impl Mul<u64> for Fraction {
    type Output = Fraction;
    fn mul(self, rhs: u64) -> Fraction {
        self * Fraction::new(rhs, 1)
    }
}

impl PartialOrd for Fraction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fraction {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.num as u128 * other.den as u128).cmp(&(other.num as u128 * self.den as u128))
    }
}

impl fmt::Display for Fraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

pub fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    if a == 0 { 1 } else { a }
}

pub fn lcm(a: u64, b: u64) -> u64 {
    a / gcd(a, b) * b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_reduces() {
        let third = Fraction::new(1, 12) * 4;
        assert_eq!(third, Fraction::new(1, 3));
        assert_eq!(
            Fraction::new(1, 4) + Fraction::new(1, 8),
            Fraction::new(3, 8)
        );
        assert_eq!(
            Fraction::new(1, 2) - Fraction::new(1, 8),
            Fraction::new(3, 8)
        );
        assert_eq!(Fraction::new(0, 7), Fraction::ZERO);
    }

    #[test]
    fn ordering() {
        assert!(Fraction::new(1, 3) > Fraction::new(1, 4));
        assert_eq!(
            Fraction::new(2, 6).cmp(&Fraction::new(1, 3)),
            Ordering::Equal
        );
    }

    #[test]
    #[should_panic]
    fn negative_subtraction_panics() {
        let _ = Fraction::new(1, 8) - Fraction::new(1, 4);
    }
}
