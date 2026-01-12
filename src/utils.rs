use crate::Cursor;

use lazy_static::lazy_static;
use regex::Regex;
use scraper::Html;
use stacked_errors::{
	bail,
	Result,
};

lazy_static! {
	pub static ref RE_DOMAIN: Regex = Regex::new(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$").unwrap();
}

/// `Attachment` object to store number attachment data and corresponding file name
#[derive(Debug)]
pub struct Attachment {
	pub data: Cursor<Vec<u8>>,
	pub name: String,
}

/// Pass any text here to be validated as HTML, breaks on validation errors
pub fn validate (text: &str) -> Result<&str> {
	// Technically full validation is not needed nor required here, all text after validation
	// is used in Telegram messages as RAW text enclosed in `pre`/`code` tags, so the only reason
	// for this check is to make sure there's no dangling closing tags in the text that might
	// break Telegram message formatting
	let fragment = Html::parse_fragment(text);
	if !fragment.errors.is_empty() {
		bail!(fragment.errors.join("\n"));
	}
	Ok(text)
}
