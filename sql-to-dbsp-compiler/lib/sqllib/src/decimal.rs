// Newtype for rust_decimal::Decimal because it doesn't implement Encode and
// Decode

use ::serde::{Deserialize, Serialize};
use bincode::{Decode, Encode};
use num::{One, Signed, Zero};
use size_of::*;

use rust_decimal::{
    prelude::{FromPrimitive, ToPrimitive},
    Decimal as RustDecimal, MathematicalOps,
};
use std::{
    fmt::{Display, Formatter},
    ops::{Add, Div, Mul, Neg, Rem, Sub},
    str::FromStr,
};

#[derive(
    Copy,
    Default,
    Eq,
    Ord,
    Clone,
    Hash,
    PartialEq,
    PartialOrd,
    SizeOf,
    Serialize,
    Deserialize,
    Debug,
    Encode,
    Decode,
)]
pub struct Decimal(#[bincode(with_serde)] RustDecimal);

impl Decimal {
    pub fn new(num: i64, scale: u32) -> Decimal {
        Self(RustDecimal::new(num, scale))
    }

    pub fn abs(&self) -> Self {
        Self(self.0.abs())
    }

    pub fn floor(&self) -> Self {
        Self(self.0.floor())
    }

    pub fn ceil(&self) -> Self {
        Self(self.0.ceil())
    }

    pub fn rescale(&mut self, scale: u32) {
        self.0.rescale(scale);
    }

    pub fn round_dp(&self, dp: u32) -> Decimal {
        Self(self.0.round_dp(dp))
    }

    pub fn signum(&self) -> Self {
        Self(self.0.signum())
    }

    pub fn trunc_with_scale(&self, scale: u32) -> Decimal {
        Self(self.0.trunc_with_scale(scale))
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn exp(&self) -> Decimal {
        Self(self.0.exp())
    }

    pub fn checked_exp(&self) -> Option<Decimal> {
        self.0.checked_exp().map(|x| Self(x))
    }

    pub fn exp_with_tolerance(&self, tolerance: Decimal) -> Decimal {
        Self(self.0.exp_with_tolerance(tolerance.0))
    }

    pub fn checked_exp_with_tolerance(&self, tolerance: Decimal) -> Option<Decimal> {
        self.0
            .checked_exp_with_tolerance(tolerance.0)
            .map(|x| Self(x))
    }

    pub fn powi(&self, exp: i64) -> Decimal {
        Self(self.0.powi(exp))
    }

    pub fn checked_powi(&self, exp: i64) -> Option<Decimal> {
        self.0.checked_powi(exp).map(|x| Self(x))
    }

    pub fn powu(&self, exp: u64) -> Decimal {
        Self(self.0.powu(exp))
    }

    pub fn checked_powu(&self, exp: u64) -> Option<Decimal> {
        self.0.checked_powu(exp).map(|x| Self(x))
    }

    pub fn powf(&self, exp: f64) -> Decimal {
        Self(self.0.powf(exp))
    }

    pub fn checked_powf(&self, exp: f64) -> Option<Decimal> {
        self.0.checked_powf(exp).map(|x| Self(x))
    }

    pub fn powd(&self, exp: Decimal) -> Decimal {
        Self(self.0.powd(exp.0))
    }

    pub fn checked_powd(&self, exp: Decimal) -> Option<Decimal> {
        self.0.checked_powd(exp.0).map(|x| Self(x))
    }

    pub fn sqrt(&self) -> Option<Decimal> {
        self.0.sqrt().map(|x| Self(x))
    }

    pub fn ln(&self) -> Decimal {
        Self(self.0.ln())
    }

    pub fn checked_ln(&self) -> Option<Decimal> {
        self.0.checked_ln().map(|x| Self(x))
    }

    pub fn log10(&self) -> Decimal {
        Self(self.0.log10())
    }

    pub fn checked_log10(&self) -> Option<Decimal> {
        self.0.checked_log10().map(|x| Self(x))
    }

    pub fn erf(&self) -> Decimal {
        Self(self.0.erf())
    }

    pub fn norm_cdf(&self) -> Decimal {
        Self(self.0.norm_cdf())
    }

    pub fn norm_pdf(&self) -> Decimal {
        Self(self.0.norm_pdf())
    }

    pub fn checked_norm_pdf(&self) -> Option<Decimal> {
        self.0.checked_norm_pdf().map(|x| Self(x))
    }

    pub fn sin(&self) -> Decimal {
        Self(self.0.sin())
    }

    pub fn checked_sin(&self) -> Option<Decimal> {
        self.0.checked_sin().map(|x| Self(x))
    }

    pub fn cos(&self) -> Decimal {
        Self(self.0.cos())
    }

    pub fn checked_cos(&self) -> Option<Decimal> {
        self.0.checked_cos().map(|x| Self(x))
    }

    pub fn tan(&self) -> Decimal {
        Self(self.0.tan())
    }

    pub fn checked_tan(&self) -> Option<Decimal> {
        self.0.checked_tan().map(|x| Self(x))
    }
}

impl Display for Decimal {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        self.0.fmt(f)
    }
}

impl FromPrimitive for Decimal {
    fn from_i64(n: i64) -> Option<Decimal> {
        RustDecimal::from_i64(n).map(|d| Self(d))
    }

    fn from_u64(n: u64) -> Option<Decimal> {
        RustDecimal::from_u64(n).map(|d| Self(d))
    }

    fn from_u128(n: u128) -> Option<Decimal> {
        RustDecimal::from_u128(n).map(|d| Self(d))
    }

    fn from_i128(n: i128) -> Option<Decimal> {
        RustDecimal::from_i128(n).map(|d| Self(d))
    }

    fn from_f64(n: f64) -> Option<Decimal> {
        RustDecimal::from_f64(n).map(|d| Self(d))
    }
}

impl ToPrimitive for Decimal {
    fn to_i64(&self) -> Option<i64> {
        self.0.to_i64()
    }
    fn to_u64(&self) -> Option<u64> {
        self.0.to_u64()
    }
    fn to_i128(&self) -> Option<i128> {
        self.0.to_i128()
    }
    fn to_u128(&self) -> Option<u128> {
        self.0.to_u128()
    }
    fn to_f64(&self) -> Option<f64> {
        self.0.to_f64()
    }
}

impl FromStr for Decimal {
    type Err = <RustDecimal as FromStr>::Err;

    fn from_str(value: &str) -> Result<Decimal, <RustDecimal as FromStr>::Err> {
        Ok(Decimal(RustDecimal::from_str(value)?))
    }
}

impl Add for Decimal {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Decimal {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Mul for Decimal {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self(self.0 * rhs.0)
    }
}

impl Div for Decimal {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Self(self.0 / rhs.0)
    }
}

impl Rem for Decimal {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self::Output {
        Self(self.0 % rhs.0)
    }
}

impl Neg for Decimal {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Zero for Decimal {
    fn zero() -> Self {
        Decimal(RustDecimal::ZERO)
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl One for Decimal {
    fn one() -> Self {
        Decimal(RustDecimal::ONE)
    }
}
