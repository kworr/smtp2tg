//! Exercise the production mail handler with a recording transport. Including
//! the module here provides a test seam for its private Telegram dependency
//! without changing production visibility or making HTTP requests.

#[path = "../src/mail.rs"]
mod mail;

use smtp2tg::utils;
use std::io::Cursor;

use mailin_embedded::{Handler, response::{INTERNAL_ERROR, OK}};
use stacked_errors::{Result, StackableErr};

#[derive(Debug, PartialEq)]
enum Delivery {
	Debug(String),
	Text(String),
	Files(Vec<(String, Vec<u8>)>, String),
}

mod telegram {
	use super::{Delivery, utils::Attachment};
	use std::{cell::RefCell, collections::HashMap};
	use stacked_errors::{Result, StackableErr};
	use tgbot::types::ChatPeerId;

	thread_local! {
		// Handler::data_end drives its future on the calling test thread.
		static DELIVERIES: RefCell<Vec<Delivery>> = const { RefCell::new(Vec::new()) };
	}

	pub fn take_deliveries () -> Vec<Delivery> {
		DELIVERIES.with(|items| std::mem::take(&mut *items.borrow_mut()))
	}

	#[derive(Debug)]
	pub struct TelegramTransport {
		pub default: ChatPeerId,
		recipients: HashMap<String, ChatPeerId>,
	}

	impl TelegramTransport {
		pub fn new (_api_key: String, recipients: HashMap<String, i64>, settings: &config::Config) -> Result<Self> {
			Ok(Self {
				default: settings.get_int("default").stack()?.into(),
				recipients: recipients.into_iter().map(|(name, id)| (name, id.into())).collect(),
			})
		}

		pub fn get (&self, name: &str) -> Result<&ChatPeerId> {
			self.recipients.get(&name.to_lowercase()).context("Unknown test recipient")
		}

		pub async fn debug (&self, msg: &str) -> Result<()> {
			DELIVERIES.with(|items| items.borrow_mut().push(Delivery::Debug(msg.into())));
			Ok(())
		}

		pub async fn send (&self, _to: &ChatPeerId, msg: &str) -> Result<()> {
			DELIVERIES.with(|items| items.borrow_mut().push(Delivery::Text(msg.into())));
			Ok(())
		}

		pub async fn sendgroup (&self, _to: &ChatPeerId, media: Vec<Attachment>, msg: &str) -> Result<()> {
			let files = media.into_iter().map(|file| (file.name, file.data.into_inner())).collect();
			DELIVERIES.with(|items| items.borrow_mut().push(Delivery::Files(files, msg.into())));
			Ok(())
		}
	}
}

fn server () -> Result<mail::MailServer> {
	telegram::take_deliveries();
	let settings = config::Config::builder()
		.add_source(config::File::from_str(r#"
			api_key = "test-api-key"
			api_gateway = "https://api.telegram.org"
			default = 0
			fields = []
			domains = ["example.com"]
			[recipients]
			"recipient@example.com" = 1
		"#, config::FileFormat::Toml))
		.build().stack()?;
	mail::MailServer::new(settings)
}

fn relay (message: &str) -> Result<Vec<Delivery>> {
	let mut server = server()?;
	assert_eq!(server.data_start("example.com", "sender@example.com", true,
		&["recipient@example.com".into()]), OK);
	server.data(message.as_bytes()).stack()?;
	assert_eq!(server.data_end(), OK);
	Ok(telegram::take_deliveries())
}

fn caption (body: &str) -> String {
	format!("<blockquote expandable>\n</blockquote><pre>{body}</pre>")
}

fn multipart (kind: &str, parts: &[&str]) -> String {
	format!("MIME-Version: 1.0\r\nContent-Type: multipart/{kind}; boundary=parts\r\n\r\n--parts\r\n{}\r\n--parts--\r\n",
		parts.join("\r\n--parts\r\n"))
}

const TEXT: &str = "Content-Type: text/plain; charset=utf-8\r\n\r\nPlain body";
const HTML: &str = "Content-Type: text/html; charset=utf-8\r\n\r\n<p>HTML body</p>";

#[test]
fn short_text_is_inlined_and_normalized () -> Result<()> {
	let deliveries = relay("Content-Type: text/plain\r\n\r\nFirst & <second>\r\nThird")?;
	assert_eq!(deliveries, vec![
		Delivery::Text(caption("First &amp; &lt;second&gt;\nThird")),
	]);
	Ok(())
}

#[test]
fn alternative_html_is_not_sent_as_a_duplicate_attachment () -> Result<()> {
	let deliveries = relay(&multipart("alternative", &[TEXT, HTML]))?;
	assert_eq!(deliveries, vec![Delivery::Text(caption("Plain body"))]);
	Ok(())
}

#[test]
fn html_only_message_uses_the_parsers_text_conversion () -> Result<()> {
	assert_eq!(relay(HTML)?, vec![
		Delivery::Text(caption("HTML body\n")),
	]);
	Ok(())
}

#[test]
fn remaining_text_parts_are_sent_once_in_order () -> Result<()> {
	let second = "Content-Type: text/plain\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nSecond=20body";
	let third = "Content-Type: text/plain\r\n\r\nThird body";
	assert_eq!(relay(&multipart("mixed", &[TEXT, second, third]))?, vec![
		Delivery::Files(vec![
			("Attachment.txt".into(), b"Second body".to_vec()),
			("Attachment.txt".into(), b"Third body".to_vec()),
		], caption("Plain body")),
	]);
	Ok(())
}

#[test]
fn text_at_the_message_limit_is_sent_as_a_decoded_attachment () -> Result<()> {
	// The existing contract requires the complete escaped message to be <4096
	// bytes. Exercise both sides so the first text iterator item cannot be lost.
	let capacity = 4096 - caption("").len();
	for size in [capacity - 1, capacity] {
		let body = "x".repeat(size);
		let message = format!("Content-Type: text/plain\r\n\r\n{body}");
		let deliveries = relay(&message)?;
		assert_eq!(deliveries.len(), 1);
		let expected = if size < capacity {
			Delivery::Text(caption(&body))
		} else {
			Delivery::Files(vec![("Attachment.txt".into(), body.into_bytes())], caption(""))
		};
		assert_eq!(deliveries[0], expected, "body size: {size}");
	}
	Ok(())
}

#[test]
fn attachment_payloads_are_transfer_decoded_and_keep_content_type_names () -> Result<()> {
	let binary = "Content-Type: application/octet-stream; name=\"payload.bin\"\r\nContent-Disposition: attachment\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC/w==";
	let quoted = "Content-Type: application/octet-stream; name=\"notes.txt\"\r\nContent-Disposition: attachment\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\ncaf=C3=A9=0Asecond=3Dline";
	assert_eq!(relay(&multipart("mixed", &[TEXT, binary, quoted]))?, vec![
		Delivery::Files(vec![
			("payload.bin".into(), vec![0, 1, 2, 255]),
			("notes.txt".into(), "café\nsecond=line".as_bytes().to_vec()),
		], caption("Plain body")),
	]);
	Ok(())
}

#[test]
fn unnamed_attachment_uses_the_fallback_filename () -> Result<()> {
	let attachment = "Content-Type: application/octet-stream\r\nContent-Disposition: attachment\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=";
	assert_eq!(relay(&multipart("mixed", &[attachment]))?, vec![
		Delivery::Files(vec![("Attachment.txt".into(), b"hello".to_vec())], caption("")),
	]);
	Ok(())
}

#[test]
fn attachment_filename_is_preserved_for_case_insensitive_mime_headers () -> Result<()> {
	// MIME field names are case insensitive. The old parser's Header::name()
	// returned canonical names; raw_name() now preserves their original case.
	for header in ["Content-Type", "content-type", "CONTENT-TYPE", "cOnTeNt-TyPe"] {
		let attachment = format!("{header}: application/octet-stream; name=\"report.bin\"\r\nContent-Disposition: attachment\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC/w==");
		assert_eq!(relay(&multipart("mixed", &[&attachment]))?, vec![
			Delivery::Files(vec![("report.bin".into(), vec![0, 1, 2, 255])], caption("")),
		], "MIME header: {header}");
	}
	Ok(())
}

#[test]
fn nested_alternatives_and_files_use_separate_iterators () -> Result<()> {
	let alternative = multipart("alternative", &[TEXT, HTML]).replace("parts", "inner");
	let extra = "Content-Type: text/plain\r\n\r\nExtra body";
	let attachment = "Content-Type: application/octet-stream; name=\"file.bin\"\r\nContent-Disposition: attachment\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC/w==";
	assert_eq!(relay(&multipart("mixed", &[&alternative, extra, attachment]))?, vec![
		Delivery::Files(vec![
			("Attachment.txt".into(), b"Extra body".to_vec()),
			("file.bin".into(), vec![0, 1, 2, 255]),
		], caption("Plain body")),
	]);
	Ok(())
}

#[test]
fn empty_text_body_does_not_create_a_spurious_attachment () -> Result<()> {
	assert_eq!(relay("Content-Type: text/plain\r\n\r\n")?, vec![
		Delivery::Text(caption("")),
	]);
	Ok(())
}

#[test]
fn forbidden_closing_tag_stops_delivery () -> Result<()> {
	let mut server = server()?;
	server.data_start("example.com", "sender@example.com", false,
		&["recipient@example.com".into()]);
	server.data(b"Content-Type: text/plain\r\n\r\nUnsafe </pre> body").stack()?;
	assert_eq!(server.data_end(), INTERNAL_ERROR);
	let deliveries = telegram::take_deliveries();
	assert_eq!(deliveries.len(), 1);
	assert!(matches!(&deliveries[0], Delivery::Debug(message)
		if message.starts_with("Sending emails failed:")));
	Ok(())
}
