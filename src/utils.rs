use crate::Cursor;

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
	pub static ref RE_SPECIAL: Regex = Regex::new(r"([\-_*\[\]()~`>#+|{}\.!])").unwrap();
	pub static ref RE_DOMAIN: Regex = Regex::new(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$").unwrap();
}

/// `Attachment` object to store number attachment data and corresponding file name
#[derive(Debug)]
pub struct Attachment {
	pub data: Cursor<Vec<u8>>,
	pub name: String,
}
