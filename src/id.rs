use derive_more::{Add, AddAssign, Deref, Display, From, Into};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::ops::{Add as StdAdd, AddAssign as StdAddAssign};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    Display,
    From,
    Into,
    Deref,
    Add,
    AddAssign,
    Type,
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct id(pub i32);

impl id {
    pub fn sort_pair(self, other: Self) -> (Self, Self) {
        if self <= other {
            (self, other)
        } else {
            (other, self)
        }
    }
}

impl StdAdd<i32> for id {
    type Output = Self;

    fn add(self, rhs: i32) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl StdAddAssign<i32> for id {
    fn add_assign(&mut self, rhs: i32) {
        self.0 += rhs;
    }
}
