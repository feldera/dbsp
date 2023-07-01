//! SQL String operations

#![allow(non_snake_case)]

use like::{Like, Escape};

pub fn concat_s_s(left: String, right: String) -> String {
    let result = format!("{}{}", left, right);
    result
}

pub fn concat_sN_s(left: Option<String>, right: String) -> Option<String> {
    left.map(|v| concat_s_s(v, right))
}

pub fn concat_s_sN(left: String, right: Option<String>) -> Option<String> {
    right.map(|v| concat_s_s(left, v))
}

pub fn concat_sN_sN(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(l), Some(r)) => Some(concat_s_s(l, r)),
    }
}

pub fn substring3(value: String, left: i32, count: i32) -> String {
    if count < 0 { return "".to_string() }
    else {
        // character indexes in SQL start at 1
        let start = if left < 1 { 0 } else { left - 1 };
        value.chars().skip(start as usize).take(count as usize).collect()
    }
}

pub fn substring2(value: String, left: i32) -> String {
    // character indexes in SQL start at 1
    let start = if left < 1 { 0 } else { left - 1 };
    value.chars().skip(start as usize).collect()
}

pub fn substringN3(value: Option<String>, left: i32, count: i32) -> Option<String> {
    value.map(|x| substring3(x, left, count))
}

pub fn substringN2(value: Option<String>, left: i32) -> Option<String> {
    value.map(|x| substring2(x, left))
}

pub fn trim_both_s_s(remove: String, value: String) -> String {
    // 'remove' always has exactly 1 character
    let chr = remove.chars().next().unwrap();
    value.trim_matches(chr).to_string()
}

pub fn trim_leading_s_s(remove: String, value: String) -> String {
    // 'remove' always has exactly 1 character
    let chr = remove.chars().next().unwrap();
    value.trim_start_matches(chr).to_string()
}

pub fn trim_trailing_s_s(remove: String, value: String) -> String {
    // 'remove' always has exactly 1 character
    let chr = remove.chars().next().unwrap();
    value.trim_end_matches(chr).to_string()
}

pub fn like2(value: String, pattern: String) -> bool {
    Like::<false>::like(value.as_str(), pattern.as_str()).unwrap()
}

pub fn like3(value: String, pattern: String, escape: String) -> bool {
    let escaped = pattern.as_str().escape(escape.as_str()).unwrap();
    Like::<true>::like(value.as_str(), escaped.as_str()).unwrap()
}

pub fn position(needle: String, haystack: String) -> i32 {
    let pos = haystack.find(needle.as_str());
    match pos {
        None => 0,
        Some(i) => (i + 1) as i32,
    }
}

pub fn char_length(value: String) -> i32 {
    value.chars().count() as i32
}

pub fn ascii(value: String) -> i32 {
    if value.is_empty() { 0 }
    else { value.chars().next().unwrap() as u32 as i32 }
}

pub fn chr(code: i32) -> String {
    if code < 0 { String::default() }
    else {
        let c = char::from_u32(code as u32);
        match c {
            None => String::default(),
            Some(v) => String::from(v),
        }
    }
}

pub fn repeat(value: String, count: i32) -> String {
    if count <= 0 { String::default() }
    else { value.repeat(count as usize) }
}
