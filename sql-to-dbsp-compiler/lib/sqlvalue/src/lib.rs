//! SqlValue is a dynamically-typed object that holds a value that at
//! runtime can appear in a SQL program.  These values are not used
//! during computations, but they are used to display values.  The
//! Tuple* types are used for computations, and they are converted
//! to SqlRow objects when they need to be displayed.

use dbsp::algebra::{F32, F64};
use sqllib::{casts::*, decimal::*};

#[derive(Debug)]
pub enum SqlValue {
    Int(i32),
    Long(i64),
    Str(String),
    Flt(f32),
    Dbl(f64),
    Bool(bool),
    Decimal(Decimal),

    OptInt(Option<i32>),
    OptLong(Option<i64>),
    OptStr(Option<String>),
    OptFlt(Option<f32>),
    OptDbl(Option<f64>),
    OptBool(Option<bool>),
    OptDecimal(Option<Decimal>),
}

impl From<i32> for SqlValue {
    fn from(value: i32) -> Self {
        SqlValue::Int(value)
    }
}

impl From<i64> for SqlValue {
    fn from(value: i64) -> Self {
        SqlValue::Long(value)
    }
}

impl From<bool> for SqlValue {
    fn from(value: bool) -> Self {
        SqlValue::Bool(value)
    }
}

impl From<f32> for SqlValue {
    fn from(value: f32) -> Self {
        SqlValue::Flt(value)
    }
}

impl From<f64> for SqlValue {
    fn from(value: f64) -> Self {
        SqlValue::Dbl(value)
    }
}

impl From<F32> for SqlValue {
    fn from(value: F32) -> Self {
        SqlValue::Flt(value.into_inner())
    }
}

impl From<F64> for SqlValue {
    fn from(value: F64) -> Self {
        SqlValue::Dbl(value.into_inner())
    }
}

impl From<String> for SqlValue {
    fn from(value: String) -> Self {
        SqlValue::Str(value)
    }
}

impl From<Decimal> for SqlValue {
    fn from(value: Decimal) -> Self {
        SqlValue::Decimal(value)
    }
}

impl From<Option<i32>> for SqlValue {
    fn from(value: Option<i32>) -> Self {
        SqlValue::OptInt(value)
    }
}

impl From<Option<i64>> for SqlValue {
    fn from(value: Option<i64>) -> Self {
        SqlValue::OptLong(value)
    }
}

impl From<Option<bool>> for SqlValue {
    fn from(value: Option<bool>) -> Self {
        SqlValue::OptBool(value)
    }
}

impl From<Option<f32>> for SqlValue {
    fn from(value: Option<f32>) -> Self {
        SqlValue::OptFlt(value)
    }
}

impl From<Option<f64>> for SqlValue {
    fn from(value: Option<f64>) -> Self {
        SqlValue::OptDbl(value)
    }
}

impl From<Option<F32>> for SqlValue {
    fn from(value: Option<F32>) -> Self {
        match value {
            None => SqlValue::OptFlt(None),
            Some(x) => SqlValue::OptFlt(Some(x.into_inner())),
        }
    }
}

impl From<Option<F64>> for SqlValue {
    fn from(value: Option<F64>) -> Self {
        match value {
            None => SqlValue::OptDbl(None),
            Some(x) => SqlValue::OptDbl(Some(x.into_inner())),
        }
    }
}

impl From<Option<String>> for SqlValue {
    fn from(value: Option<String>) -> Self {
        SqlValue::OptStr(value)
    }
}

impl From<Option<Decimal>> for SqlValue {
    fn from(value: Option<Decimal>) -> Self {
        SqlValue::OptDecimal(value)
    }
}

#[derive(Default)]
pub struct SqlRow {
    values: Vec<SqlValue>,
}

pub trait ToSqlRow {
    fn to_row(&self) -> SqlRow;
}

impl SqlRow {
    pub fn new() -> Self {
        SqlRow {
            values: Vec::default(),
        }
    }

    /// Output the SqlRow value in the format expected by the tests
    /// in SqlLogicTest.
    /// # Arguments
    /// - 'format': a string with characters I, R, or T, standing
    /// respectively for integers, real, or text.
    /// # Panics
    /// if format.let() != self.values.len()
    pub fn to_slt_strings(self, format: &String) -> Vec<String> {
        if self.values.len() != format.len() {
            panic!(
                "Mismatched format {} vs len {}",
                format.len(),
                self.values.len()
            )
        }
        let mut result = Vec::<String>::with_capacity(format.len());
        for elem in self.values.iter().zip(format.chars()) {
            result.push(elem.0.format_slt(&elem.1));
        }
        result
    }
}

impl SqlRow {
    pub fn push(&mut self, value: SqlValue) {
        self.values.push(value)
    }
}

pub trait SqlLogicTestFormat {
    fn format_slt(&self, arg: &char) -> String;
}

/// Convert a string result according to the SqlLogicTest rules:
/// empty strings are turned into (empty)
/// non-printable (ASCII) characters are turned into @
fn slt_translate_string(s: &String) -> String {
    if s.is_empty() {
        return String::from("(empty)");
    }
    let mut result = String::with_capacity(s.len());
    for mut c in s.chars() {
        if !(' '..='~').contains(&c) {
            c = '@';
        }
        result.push(c);
    }
    result
}

/// Format a SqlValue according to SqlLogicTest rules
/// the arg is one character of the form I - i32, R - f32, or T - String.
impl SqlLogicTestFormat for SqlValue {
    fn format_slt(&self, arg: &char) -> String {
        match (self, arg) {
            (SqlValue::Int(x), _) => format!("{}", x),
            (SqlValue::OptInt(None), _) => String::from("NULL"),
            (SqlValue::OptInt(Some(x)), _) => format!("{}", x),

            (SqlValue::Decimal(x), _) => format!("{}", x),
            (SqlValue::OptDecimal(None), _) => String::from("NULL"),
            (SqlValue::OptDecimal(Some(x)), _) => format!("{}", x),

            (SqlValue::Long(x), _) => format!("{}", x),
            (SqlValue::OptLong(None), _) => String::from("NULL"),
            (SqlValue::OptLong(Some(x)), _) => format!("{}", x),

            (SqlValue::OptFlt(None), _) => String::from("NULL"),
            (SqlValue::Flt(x), 'I') => format!("{}", *x as i32),
            (SqlValue::OptFlt(Some(x)), 'I') => format!("{}", *x as i32),
            (SqlValue::Flt(x), _) => format!("{:.3}", x),
            (SqlValue::OptFlt(Some(x)), _) => format!("{:.3}", x),

            (SqlValue::OptDbl(None), _) => String::from("NULL"),
            (SqlValue::Dbl(x), 'I') => format!("{}", *x as i32),
            (SqlValue::OptDbl(Some(x)), 'I') => format!("{}", *x as i32),
            (SqlValue::Dbl(x), _) => format!("{:.3}", x),
            (SqlValue::OptDbl(Some(x)), _) => format!("{:.3}", x),

            (SqlValue::Str(x), 'T') => slt_translate_string(x),
            (SqlValue::OptStr(None), 'T') => String::from("NULL"),
            (SqlValue::OptStr(Some(x)), 'T') => slt_translate_string(x),
            (SqlValue::OptStr(None), 'I') => String::from("NULL"),
            (SqlValue::OptStr(Some(x)), 'I') => format!("{}", cast_to_i32_s(x.clone())),

            (SqlValue::OptBool(None), _) => String::from("NULL"),
            (SqlValue::Bool(b), _) => format!("{}", b),
            (SqlValue::OptBool(Some(b)), _) => format!("{}", b),
            _ => panic!("Unexpected combination {:?} {:?}", self, arg),
        }
    }
}
