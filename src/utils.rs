use crate::Cursor;

use lazy_static::lazy_static;
use regex::Regex;
use stacked_errors::{
	bail,
	Result,
};

lazy_static! {
	pub static ref RE_DOMAIN: Regex = Regex::new(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$").unwrap();
	pub static ref RE_CLOSING: Regex = Regex::new(r"</[ \t]*(pre|code)[ \t]*>").unwrap();
}

/// `Attachment` object to store number attachment data and corresponding file name
#[derive(Debug)]
pub struct Attachment {
	pub data: Cursor<Vec<u8>>,
	pub name: String,
}

/// Pass any text here to be validated as not breaking from Telegram preformatted blocks
pub fn validate (text: &str) -> Result<&str> {
	if RE_CLOSING.is_match(text) {
		bail!("Telegram closing tag found.");
	} else {
		Ok(text)
	}
}
