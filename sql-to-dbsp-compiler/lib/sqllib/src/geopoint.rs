// I cannot use the standard geopoint object because it doesn't implement Ord

use ::serde::{Deserialize, Serialize};
use bincode::Decode;
use bincode::Encode;
use dbsp::algebra::F64;
use geo::EuclideanDistance;
use geo::Point;
use size_of::*;

#[derive(
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
pub struct GeoPoint(F64, F64);

impl GeoPoint {
    pub fn new<T, S>(left: T, right: S) -> Self
    where
        F64: From<T>,
        F64: From<S>,
    {
        Self(F64::from(left), F64::from(right))
    }

    pub fn to_point(&self) -> Point {
        Point::new(self.0.into_inner(), self.1.into_inner())
    }

    pub fn distance(&self, other: &GeoPoint) -> F64 {
        let left = self.to_point();
        let right = other.to_point();
        F64::from(left.euclidean_distance(&right))
    }
}

pub fn make_geopoint_d_d(left: F64, right: F64) -> GeoPoint {
    GeoPoint::new(left, right)
}

pub fn make_geopointN_d_d(left: F64, right: F64) -> Option<GeoPoint> {
    Some(make_geopoint_d_d(left, right))
}

pub fn make_geopointN_dN_d(left: Option<F64>, right: F64) -> Option<GeoPoint> {
    left.map(|x| make_geopoint_d_d(x, right))
}

pub fn make_geopointN_d_dN(left: F64, right: Option<F64>) -> Option<GeoPoint> {
    right.map(|x| make_geopoint_d_d(left, x))
}

pub fn make_geopointN_dN_dN(left: Option<F64>, right: Option<F64>) -> Option<GeoPoint> {
    match (left, right) {
        (None, _) => None,
        (_, None) => None,
        (Some(x), Some(y)) => make_geopointN_d_d(x, y),
    }
}
