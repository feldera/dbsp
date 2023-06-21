//! Support for SQL Timestamp and Date data types.

use crate::interval::{LongInterval, ShortInterval};
use bincode::{Decode, Encode};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};
use serde::{de::Error as _, ser::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use size_of::SizeOf;
use std::{
    fmt::{self, Debug},
    ops::Add,
};

/// Similar to a unix timestamp: a positive time interval between Jan 1 1970 and
/// the current time. The supported range is limited (e.g., up to 2038 in
/// MySQL). We use milliseconds to represent the interval.
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, SizeOf, Encode, Decode)]
pub struct Timestamp {
    // since unix epoch
    milliseconds: i64,
}

impl Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let datetime = NaiveDateTime::from_timestamp_millis(self.milliseconds).ok_or(fmt::Error)?;

        f.write_str(&datetime.format("%F %T%.f").to_string())
    }
}

/// Serialize timestamp into the `YYYY-MM-DD HH:MM:SS.fff` format, where
/// `fff` is the fractional part of a second.
impl Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let datetime =
            NaiveDateTime::from_timestamp_millis(self.milliseconds).ok_or_else(|| {
                S::Error::custom(format!(
                    "timestamp value '{}' out of range",
                    self.milliseconds
                ))
            })?;

        serializer.serialize_str(&datetime.format("%F %T%.f").to_string())
    }
}

/// Deserialize timestamp from the `YYYY-MM-DD HH:MM:SS.fff` format.
///
/// # Caveats
///
/// * The format does not include a time zone, because the `TIMESTAMP` type is
///   supposed to be timezone-agnostic.
/// * Different databases may use different default export formats for
///   timestamps.  Moreover, I suspect that binary serialization formats
///   represent timestamps as numbers (which is how they are stored inside the
///   DB), so in the future we may need a smarter implementation that supports
///   all these variants.
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let timestamp_str: &'de str = Deserialize::deserialize(deserializer)?;

        let timestamp = NaiveDateTime::parse_from_str(timestamp_str, "%F %T%.f").map_err(|e| {
            D::Error::custom(format!("invalid timestamp string '{timestamp_str}': {e}"))
        })?;

        Ok(Self::new(timestamp.timestamp_millis()))
    }
}

impl Timestamp {
    pub const fn new(milliseconds: i64) -> Self {
        Self { milliseconds }
    }

    pub fn milliseconds(&self) -> i64 {
        self.milliseconds
    }

    pub fn to_dateTime(&self) -> DateTime<Utc> {
        Utc.timestamp_opt(
            self.milliseconds / 1000,
            ((self.milliseconds % 1000i64) as u32) * 1_000_000,
        )
        .single()
        .unwrap()
    }

    pub fn from_dateTime(date: DateTime<Utc>) -> Self {
        Self {
            milliseconds: date.timestamp_millis(),
        }
    }
}

impl<T> From<T> for Timestamp
where
    i64: From<T>,
{
    fn from(value: T) -> Self {
        Self {
            milliseconds: i64::from(value),
        }
    }
}

impl Add<i64> for Timestamp {
    type Output = Self;

    fn add(self, value: i64) -> Self {
        Self {
            milliseconds: self.milliseconds + value,
        }
    }
}

pub fn plus_Timestamp_ShortInterval(left: Timestamp, right: ShortInterval) -> Timestamp {
    left.add(right.milliseconds())
}

pub fn minus_Timestamp_Timestamp_ShortInterval(left: Timestamp, right: Timestamp) -> ShortInterval {
    ShortInterval::from(left.milliseconds() - right.milliseconds())
}

pub fn minus_TimestampN_Timestamp_ShortIntervalN(
    left: Option<Timestamp>,
    right: Timestamp,
) -> Option<ShortInterval> {
    left.map(|l| minus_Timestamp_Timestamp_ShortInterval(l, right))
}

pub fn minus_Timestamp_ShortInterval_Timestamp(left: Timestamp, right: ShortInterval) -> Timestamp {
    Timestamp::new(left.milliseconds() - right.milliseconds())
}

pub fn minus_Timestamp_TimestampN_ShortIntervalN(
    left: Timestamp,
    right: Option<Timestamp>,
) -> Option<ShortInterval> {
    right.map(|r| ShortInterval::from(left.milliseconds() - r.milliseconds()))
}

pub fn minus_Timestamp_Timestamp_LongInterval(left: Timestamp, right: Timestamp) -> LongInterval {
    let ldate = left.to_dateTime();
    let rdate = right.to_dateTime();
    let ly = ldate.year();
    let lm = ldate.month() as i32;
    let ld = ldate.day() as i32;
    let lt = ldate.time();

    let ry = rdate.year();
    let mut rm = rdate.month() as i32;
    let rd = rdate.day() as i32;
    let rt = rdate.time();
    if (ld < rd) || ((ld == rd) && lt < rt) {
        // the full month is not yet elapsed
        rm += 1;
    }
    LongInterval::from((ly - ry) * 12 + lm - rm)
}

pub fn minus_TimestampN_Timestamp_LongIntervalN(
    left: Option<Timestamp>,
    right: Timestamp,
) -> Option<LongInterval> {
    left.map(|l| minus_Timestamp_Timestamp_LongInterval(l, right))
}

pub fn extract_Timestamp_year(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.year().into()
}

pub fn extract_Timestamp_yearN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_year)
}

pub fn extract_Timestamp_month(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.month().into()
}

pub fn extract_Timestamp_monthN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_month)
}

pub fn extract_Timestamp_day(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.day().into()
}

pub fn extract_Timestamp_dayN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_day)
}

pub fn extract_Timestamp_quarter(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    (date.month0() / 3 + 1).into()
}

pub fn extract_Timestamp_quarterN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_quarter)
}

pub fn extract_Timestamp_decade(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    (date.year() / 10).into()
}

pub fn extract_Timestamp_decadeN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_decade)
}

pub fn extract_Timestamp_century(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    ((date.year() + 99) / 100).into()
}

pub fn extract_Timestamp_centuryN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_century)
}

pub fn extract_Timestamp_millennium(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    ((date.year() + 999) / 1000).into()
}

pub fn extract_Timestamp_millenniumN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_millennium)
}

pub fn extract_Timestamp_isoyear(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.iso_week().year().into()
}

pub fn extract_Timestamp_isoyearN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_isoyear)
}

pub fn extract_Timestamp_week(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.iso_week().week().into()
}

pub fn extract_Timestamp_weekN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_week)
}

pub fn extract_Timestamp_dow(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    (date.weekday().num_days_from_sunday() as i64) + Date::first_day_of_week()
}

pub fn extract_Timestamp_dowN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_dow)
}

pub fn extract_Timestamp_isodow(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    (date.weekday().num_days_from_monday() as i64) + 1
}

pub fn extract_Timestamp_isodowN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_isodow)
}

pub fn extract_Timestamp_doy(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.ordinal().into()
}

pub fn extract_Timestamp_doyN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_doy)
}

pub fn extract_Timestamp_epoch(t: Timestamp) -> i64 {
    t.milliseconds() / 1000
}

pub fn extract_Timestamp_epochN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_epoch)
}

pub fn extract_Timestamp_millisecond(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    (date.second() * 1000 + date.timestamp_subsec_millis()).into()
}

pub fn extract_Timestamp_millisecondN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_millisecond)
}

pub fn extract_Timestamp_microsecond(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    (date.second() * 1_000_000 + date.timestamp_subsec_micros()).into()
}

pub fn extract_Timestamp_microsecondN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_microsecond)
}

pub fn extract_Timestamp_second(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.second().into()
}

pub fn extract_Timestamp_secondN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_second)
}

pub fn extract_Timestamp_minute(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.minute().into()
}

pub fn extract_Timestamp_minuteN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_minute)
}

pub fn extract_Timestamp_hour(value: Timestamp) -> i64 {
    let date = value.to_dateTime();
    date.hour().into()
}

pub fn extract_Timestamp_hourN(value: Option<Timestamp>) -> Option<i64> {
    value.map(extract_Timestamp_hour)
}

pub fn lt_Timestamp_Timestamp(left: Timestamp, right: Timestamp) -> bool {
    left < right
}

pub fn lt_TimestampN_Timestamp(left: Option<Timestamp>, right: Timestamp) -> Option<bool> {
    left.map(|l| l < right)
}

pub fn lt_Timestamp_TimestampN(left: Timestamp, right: Option<Timestamp>) -> Option<bool> {
    right.map(|r| left < r)
}

pub fn lt_TimestampN_TimestampN(left: Option<Timestamp>, right: Option<Timestamp>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l < r),
    }
}

pub fn gt_Timestamp_Timestamp(left: Timestamp, right: Timestamp) -> bool {
    left > right
}

pub fn gt_TimestampN_Timestamp(left: Option<Timestamp>, right: Timestamp) -> Option<bool> {
    left.map(|l| l > right)
}

pub fn gt_Timestamp_TimestampN(left: Timestamp, right: Option<Timestamp>) -> Option<bool> {
    right.map(|r| left > r)
}

pub fn gt_TimestampN_TimestampN(left: Option<Timestamp>, right: Option<Timestamp>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l > r),
    }
}

pub fn eq_Timestamp_Timestamp(left: Timestamp, right: Timestamp) -> bool {
    left == right
}

pub fn eq_TimestampN_Timestamp(left: Option<Timestamp>, right: Timestamp) -> Option<bool> {
    left.map(|l| l == right)
}

pub fn eq_Timestamp_TimestampN(left: Timestamp, right: Option<Timestamp>) -> Option<bool> {
    right.map(|r| left == r)
}

pub fn eq_TimestampN_TimestampN(left: Option<Timestamp>, right: Option<Timestamp>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l == r),
    }
}

pub fn neq_Timestamp_Timestamp(left: Timestamp, right: Timestamp) -> bool {
    left != right
}

pub fn neq_TimestampN_Timestamp(left: Option<Timestamp>, right: Timestamp) -> Option<bool> {
    left.map(|l| l != right)
}

pub fn neq_Timestamp_TimestampN(left: Timestamp, right: Option<Timestamp>) -> Option<bool> {
    right.map(|r| left != r)
}

pub fn neq_TimestampN_TimestampN(
    left: Option<Timestamp>,
    right: Option<Timestamp>,
) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l != r),
    }
}

pub fn gte_Timestamp_Timestamp(left: Timestamp, right: Timestamp) -> bool {
    left >= right
}

pub fn gte_TimestampN_Timestamp(left: Option<Timestamp>, right: Timestamp) -> Option<bool> {
    left.map(|l| l >= right)
}

pub fn gte_Timestamp_TimestampN(left: Timestamp, right: Option<Timestamp>) -> Option<bool> {
    right.map(|r| left >= r)
}

pub fn gte_TimestampN_TimestampN(
    left: Option<Timestamp>,
    right: Option<Timestamp>,
) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l >= r),
    }
}

pub fn lte_Timestamp_Timestamp(left: Timestamp, right: Timestamp) -> bool {
    left <= right
}

pub fn lte_TimestampN_Timestamp(left: Option<Timestamp>, right: Timestamp) -> Option<bool> {
    left.map(|l| l <= right)
}

pub fn lte_Timestamp_TimestampN(left: Timestamp, right: Option<Timestamp>) -> Option<bool> {
    right.map(|r| left <= r)
}

pub fn lte_TimestampN_TimestampN(
    left: Option<Timestamp>,
    right: Option<Timestamp>,
) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l <= r),
    }
}

pub fn floor_Timestamp_week(value: Timestamp) -> Timestamp {
    let wd = extract_Timestamp_dow(value) - Date::first_day_of_week();
    let ts = value.to_dateTime();
    let notime = ts
        .with_hour(0)
        .unwrap()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap();
    let notimeTs = Timestamp::from_dateTime(notime);
    let interval = ShortInterval::new(wd * 86400 * 1000);

    minus_Timestamp_ShortInterval_Timestamp(notimeTs, interval)
}

pub fn floor_TimestampN_week(value: Option<Timestamp>) -> Option<Timestamp> {
    value.map(floor_Timestamp_week)
}

////////////////////////////

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, SizeOf, Encode, Decode,
)]
pub struct Date {
    // since unix epoch
    days: i32,
}

impl Date {
    pub const fn new(days: i32) -> Self {
        Self { days }
    }

    pub fn days(&self) -> i32 {
        self.days
    }

    pub fn to_timestamp(&self) -> Timestamp {
        Timestamp::new((self.days as i64) * 86400 * 1000)
    }

    pub fn to_dateTime(&self) -> DateTime<Utc> {
        Utc.timestamp_opt(self.days() as i64 * 86400, 0)
            .single()
            .unwrap()
    }

    pub fn first_day_of_week() -> i64 {
        // This should depend on the SQL dialect, but the calcite
        // optimizer seems to imply this for all dialects.
        1
    }
}

impl<T> From<T> for Date
where
    i32: From<T>,
{
    fn from(value: T) -> Self {
        Self {
            days: i32::from(value),
        }
    }
}

/// Serialize date into the `YYYY-MM-DD` format
impl Serialize for Date {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let millis = (self.days as i64) * 86400 * 1000;
        let datetime = NaiveDateTime::from_timestamp_millis(millis)
            .ok_or_else(|| S::Error::custom(format!("date value '{}' out of range", self.days)))?;

        serializer.serialize_str(&datetime.format("%F").to_string())
    }
}

/// Deserialize date from the `YYYY-MM-DD` format.
impl<'de> Deserialize<'de> for Date {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let str: &'de str = Deserialize::deserialize(deserializer)?;
        let date = NaiveDate::parse_from_str(str, "%Y-%m-%d")
            .map_err(|e| D::Error::custom(format!("invalid date string '{str}': {e}")))?;
        Ok(Self::new(
            (date.and_time(NaiveTime::default()).timestamp() / 86400) as i32,
        ))
    }
}

pub fn lt_date_date(left: Date, right: Date) -> bool {
    left < right
}

pub fn lt_dateN_date(left: Option<Date>, right: Date) -> Option<bool> {
    left.map(|l| l < right)
}

pub fn lt_date_dateN(left: Date, right: Option<Date>) -> Option<bool> {
    right.map(|r| left < r)
}

pub fn lt_dateN_dateN(left: Option<Date>, right: Option<Date>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l < r),
    }
}

pub fn gt_date_date(left: Date, right: Date) -> bool {
    left > right
}

pub fn gt_dateN_date(left: Option<Date>, right: Date) -> Option<bool> {
    left.map(|l| l > right)
}

pub fn gt_date_dateN(left: Date, right: Option<Date>) -> Option<bool> {
    right.map(|r| left > r)
}

pub fn gt_dateN_dateN(left: Option<Date>, right: Option<Date>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l > r),
    }
}

pub fn eq_date_date(left: Date, right: Date) -> bool {
    left == right
}

pub fn eq_dateN_date(left: Option<Date>, right: Date) -> Option<bool> {
    left.map(|l| l == right)
}

pub fn eq_date_dateN(left: Date, right: Option<Date>) -> Option<bool> {
    right.map(|r| left == r)
}

pub fn eq_dateN_dateN(left: Option<Date>, right: Option<Date>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l == r),
    }
}

pub fn neq_date_date(left: Date, right: Date) -> bool {
    left != right
}

pub fn neq_dateN_date(left: Option<Date>, right: Date) -> Option<bool> {
    left.map(|l| l != right)
}

pub fn neq_date_dateN(left: Date, right: Option<Date>) -> Option<bool> {
    right.map(|r| left != r)
}

pub fn neq_dateN_dateN(left: Option<Date>, right: Option<Date>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l != r),
    }
}

pub fn gte_date_date(left: Date, right: Date) -> bool {
    left >= right
}

pub fn gte_dateN_date(left: Option<Date>, right: Date) -> Option<bool> {
    left.map(|l| l >= right)
}

pub fn gte_date_dateN(left: Date, right: Option<Date>) -> Option<bool> {
    right.map(|r| left >= r)
}

pub fn gte_dateN_dateN(left: Option<Date>, right: Option<Date>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l >= r),
    }
}

pub fn lte_date_date(left: Date, right: Date) -> bool {
    left <= right
}

pub fn lte_dateN_date(left: Option<Date>, right: Date) -> Option<bool> {
    left.map(|l| l <= right)
}

pub fn lte_date_dateN(left: Date, right: Option<Date>) -> Option<bool> {
    right.map(|r| left <= r)
}

pub fn lte_dateN_dateN(left: Option<Date>, right: Option<Date>) -> Option<bool> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(l <= r),
    }
}

pub fn minus_date_date_LongInterval(left: Date, right: Date) -> LongInterval {
    let ld = left.to_dateTime();
    let rd = right.to_dateTime();
    let ly = ld.year();
    let lm = ld.month() as i32;
    let ry = rd.year();
    let rm = rd.month() as i32;
    LongInterval::from((ly - ry) * 12 + lm - rm)
}

pub fn minus_dateN_date_LongIntervaNl(left: Option<Date>, right: Date) -> Option<LongInterval> {
    left.map(|x| LongInterval::new(x.days() - right.days()))
}

pub fn minus_date_dateN_LongInterval(left: Date, right: Option<Date>) -> Option<LongInterval> {
    right.map(|x| LongInterval::new(left.days() - x.days()))
}

pub fn minus_dateN_dateN_LongIntervalN(
    left: Option<Date>,
    right: Option<Date>,
) -> Option<LongInterval> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(x), Some(y)) => Some(LongInterval::new(x.days() - y.days())),
    }
}

pub fn minus_date_date_ShortInterval(left: Date, right: Date) -> ShortInterval {
    let ld = left.days() as i64;
    let rd = right.days() as i64;
    ShortInterval::new((ld - rd) * 86400 * 1000)
}

pub fn minus_dateN_date_ShortIntervalN(left: Option<Date>, right: Date) -> Option<ShortInterval> {
    left.map(|x| minus_date_date_ShortInterval(x, right))
}

pub fn minus_date_dateN_ShortIntervalN(left: Date, right: Option<Date>) -> Option<ShortInterval> {
    right.map(|x| minus_date_date_ShortInterval(left, x))
}

pub fn minus_dateN_dateN_ShortIntervalN(
    left: Option<Date>,
    right: Option<Date>,
) -> Option<ShortInterval> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(x), Some(y)) => Some(minus_date_date_ShortInterval(x, y)),
    }
}

pub fn extract_Date_year(value: Date) -> i64 {
    let date = value.to_dateTime();
    date.year().into()
}

pub fn extract_Date_yearN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_year)
}

pub fn extract_Date_month(value: Date) -> i64 {
    let date = value.to_dateTime();
    date.month().into()
}

pub fn extract_Date_monthN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_month)
}

pub fn extract_Date_day(value: Date) -> i64 {
    let date = value.to_dateTime();
    date.day().into()
}

pub fn extract_Date_dayN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_day)
}

pub fn extract_Date_quarter(value: Date) -> i64 {
    let date = value.to_dateTime();
    (date.month0() / 3 + 1).into()
}

pub fn extract_Date_quarterN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_quarter)
}

pub fn extract_Date_decade(value: Date) -> i64 {
    let date = value.to_dateTime();
    (date.year() / 10).into()
}

pub fn extract_Date_decadeN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_decade)
}

pub fn extract_Date_century(value: Date) -> i64 {
    let date = value.to_dateTime();
    ((date.year() + 99) / 100).into()
}

pub fn extract_Date_centuryN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_century)
}

pub fn extract_Date_millennium(value: Date) -> i64 {
    let date = value.to_dateTime();
    ((date.year() + 999) / 1000).into()
}

pub fn extract_Date_millenniumN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_millennium)
}

pub fn extract_Date_isoyear(value: Date) -> i64 {
    let date = value.to_dateTime();
    date.iso_week().year().into()
}

pub fn extract_Date_isoyearN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_isoyear)
}

pub fn extract_Date_week(value: Date) -> i64 {
    let date = value.to_dateTime();
    date.iso_week().week().into()
}

pub fn extract_Date_weekN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_week)
}

pub fn extract_Date_dow(value: Date) -> i64 {
    let date = value.to_dateTime();
    (date.weekday().num_days_from_sunday() as i64) + Date::first_day_of_week()
}

pub fn extract_Date_dowN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_dow)
}

pub fn extract_Date_isodow(value: Date) -> i64 {
    let date = value.to_dateTime();
    (date.weekday().num_days_from_monday() as i64) + 1
}

pub fn extract_Date_isodowN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_isodow)
}

pub fn extract_Date_doy(value: Date) -> i64 {
    let date = value.to_dateTime();
    date.ordinal().into()
}

pub fn extract_Date_doyN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_doy)
}

pub fn extract_Date_epoch(value: Date) -> i64 {
    (value.days() as i64) * 86400
}

pub fn extract_Date_epochN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_epoch)
}

pub fn extract_Date_second(_value: Date) -> i64 {
    0
}

pub fn extract_Date_secondN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_second)
}

pub fn extract_Date_minute(_value: Date) -> i64 {
    0
}

pub fn extract_Date_minuteN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_minute)
}

pub fn extract_Date_hour(_value: Date) -> i64 {
    0
}

pub fn extract_Date_hourN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_hour)
}

pub fn extract_Date_millisecond(_value: Date) -> i64 {
    0
}

pub fn extract_Date_millisecondN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_millisecond)
}

pub fn extract_Date_microsecond(_value: Date) -> i64 {
    0
}

pub fn extract_Date_microsecondN(value: Option<Date>) -> Option<i64> {
    value.map(extract_Date_microsecond)
}
