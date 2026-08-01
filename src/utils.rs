use crate::Cursor;

use std::{
	borrow::Cow,
	sync::LazyLock,
};

use html_escape::encode_text;
use regex::{
	Regex,
	RegexBuilder,
};
use stacked_errors::{
	bail,
	Result,
};

pub static RE_DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$").expect("Invalid domain regex")
});
pub static RE_CLOSING: LazyLock<Regex> = LazyLock::new(|| {
	RegexBuilder::new(r"</[ \t]*(pre|code)[ \t]*>")
		.case_insensitive(true).build().expect("Invalid closing tag regex")
});

/// Stores binary attachment data and metadata for Telegram messages.
/// The data is wrapped in a `Cursor<Vec<u8>>` for efficient streaming,
/// while `name` holds the filename or display name of the attachment.
#[derive(Debug)]
pub struct Attachment {
	pub data: Cursor<Vec<u8>>,
	pub name: String,
}

/// Pass any text here to be validated as not breaking from Telegram preformatted blocks
/// escape all HTML chars afterwards
pub fn validate <'a>(text: &'a str) -> Result<Cow<'a, str>> {
	if RE_CLOSING.is_match(text) {
		bail!("Telegram closing tag found.");
	} else {
		Ok(encode_text(text))
	}
}
