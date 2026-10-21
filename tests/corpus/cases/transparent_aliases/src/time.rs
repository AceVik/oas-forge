/// A point in time, in UTC.
pub type Stamp = chrono::DateTime<chrono::Utc>;

pub type Seats = std::collections::HashMap<usize, Seat>;

/// Several pages at once.
pub type Paged<T> = Page<T>;

/// Only in this file: `u8`.
type Code = u8;
