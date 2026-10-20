lazy_static::lazy_static! {
    pub static ref HANDLE: regex::Regex = regex::Regex::new(r"^@[a-z0-9_]+$").unwrap();
}
