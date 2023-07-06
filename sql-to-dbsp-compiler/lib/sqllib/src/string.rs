//! SQL String operations

#![allow(non_snake_case)]

use like::{Escape, Like};

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
    if count < 0 {
        return "".to_string();
    } else {
        // character indexes in SQL start at 1
        let start = if left < 1 { 0 } else { left - 1 };
        value
            .chars()
            .skip(start as usize)
            .take(count as usize)
            .collect()
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

pub fn trim_both(remove: String, value: String) -> String {
    // 'remove' always has exactly 1 character
    let chr = remove.chars().next().unwrap();
    value.trim_matches(chr).to_string()
}

pub fn trim_leading(remove: String, value: String) -> String {
    // 'remove' always has exactly 1 character
    let chr = remove.chars().next().unwrap();
    value.trim_start_matches(chr).to_string()
}

pub fn trim_trailing(remove: String, value: String) -> String {
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
