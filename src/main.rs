//! Simple SMTP-to-Telegram gateway. Can parse email and send them as telegram
//! messages to specified chats, generally you specify which email address is
//! available in configuration, everything else is sent to default address.

use anyhow::Result;
use async_std::{
	fs::metadata,
	io::Error,
	task,
};
use just_getopt::{
	OptFlags,
	OptSpecs,
	OptValueType,
};
use lazy_static::lazy_static;
use mailin_embedded::{
	Response,
	response::*,
};
use regex::Regex;
use teloxide::{
	Bot,
	prelude::{
		Requester,
		RequesterExt,
	},
	types::{
		ChatId,
		InputMedia,
		Message,
		ParseMode::MarkdownV2,
	},
};
use thiserror::Error;

use std::{
	borrow::Cow,
	collections::{
		HashMap,
		HashSet,
	},
	os::unix::fs::PermissionsExt,
	path::Path,
	vec::Vec,
};

#[derive(Error, Debug)]
pub enum MyError {
	#[error("Failed to parse mail")]
	BadMail,
	#[error("Missing default address in recipient table")]
	NoDefault,
	#[error("No headers found")]
	NoHeaders,
	#[error("No recipient addresses")]
	NoRecipient,
	#[error("Failed to extract text from message")]
	NoText,
	#[error(transparent)]
	RequestError(#[from] teloxide::RequestError),
}

/// `SomeHeaders` object to store data through SMTP session
#[derive(Clone, Debug)]
struct SomeHeaders {
	from: String,
	to: Vec<String>,
}

/// `TelegramTransport` Central object with TG api and configuration
#[derive(Clone)]
struct TelegramTransport {
	data: Vec<u8>,
	headers: Option<SomeHeaders>,
	recipients: HashMap<String, ChatId>,
	relay: bool,
	tg: teloxide::adaptors::DefaultParseMode<teloxide::adaptors::Throttle<Bot>>,
	fields: HashSet<String>,
}

lazy_static! {
	static ref RE_SPECIAL: Regex = Regex::new(r"([\-_*\[\]()~`>#+|{}\.!])").unwrap();
}

/// Encodes special HTML entities to prevent them interfering with Telegram HTML
fn encode (text: &str) -> Cow<'_, str> {
	RE_SPECIAL.replace_all(text, "\\$1")
}

#[cfg(test)]
mod tests {
	use crate::encode;

	#[test]
	fn check_regex () {
		let res = encode("-_*[]()~`>#+|{}.!");
		assert_eq!(res, "\\-\\_\\*\\[\\]\\(\\)\\~\\`\\>\\#\\+\\|\\{\\}\\.\\!");
	}
}

impl TelegramTransport {
	/// Initialize API and read configuration
	fn new(settings: config::Config) -> TelegramTransport {
		let tg = Bot::new(settings.get_string("api_key")
			.expect("[smtp2tg.toml] missing \"api_key\" parameter.\n"))
			.throttle(teloxide::adaptors::throttle::Limits::default())
			.parse_mode(MarkdownV2);
		let recipients: HashMap<String, ChatId> = settings.get_table("recipients")
			.expect("[smtp2tg.toml] missing table \"recipients\".\n")
			.into_iter().map(|(a, b)| (a, ChatId (b.into_int()
				.expect("[smtp2tg.toml] \"recipient\" table values should be integers.\n")
				))).collect();
		if !recipients.contains_key("_") {
			eprintln!("[smtp2tg.toml] \"recipient\" table misses \"default_recipient\".\n");
			panic!("no default recipient");
		}
		let fields = HashSet::<String>::from_iter(settings.get_array("fields")
			.expect("[smtp2tg.toml] \"fields\" should be an array")
			.iter().map(|x| x.clone().into_string().expect("should be strings")));
		let value = settings.get_string("unknown");
		let relay = match value {
			Ok(value) => {
				match value.as_str() {
					"relay" => true,
					"deny" => false,
					_ => {
						eprintln!("[smtp2tg.toml] \"unknown\" should be either \"relay\" or \"deny\".\n");
						panic!("bad setting");
					},
				}
			},
			Err(err) => {
				eprintln!("[smtp2tg.toml] can't get \"unknown\":\n {err:?}\n");
				panic!("bad setting");
			},
		};

		TelegramTransport {
			data: vec!(),
			headers: None,
			recipients,
			relay,
			tg,
			fields,
		}
	}

	/// Send message to default user, used for debug/log/info purposes
	async fn debug (&self, msg: &str) -> Result<Message, MyError> {
		Ok(self.tg.send_message(*self.recipients.get("_").ok_or(MyError::NoDefault)?, encode(msg)).await?)
	}

	/// Send message to specified user
	async fn send <S> (&self, to: &ChatId, msg: S) -> Result<Message, MyError>
	where S: Into<String> {
		Ok(self.tg.send_message(*to, msg).await?)
	}

	/// Attempt to deliver one message
	async fn relay_mail (&self) -> Result<(), MyError> {
		if let Some(headers) = &self.headers {
			let mail = mail_parser::MessageParser::new().parse(&self.data)
				.ok_or(MyError::BadMail)?;

			// Adding all known addresses to recipient list, for anyone else adding default
			// Also if list is empty also adding default
			let mut rcpt: HashSet<&ChatId> = HashSet::new();
			if headers.to.is_empty() {
				return Err(MyError::NoRecipient);
			}
			for item in &headers.to {
				match self.recipients.get(item) {
					Some(addr) => rcpt.insert(addr),
					None => {
						self.debug(&format!("Recipient [{item}] not found.")).await?;
						rcpt.insert(self.recipients.get("_")
							.ok_or(MyError::NoDefault)?)
					}
				};
			};
			if rcpt.is_empty() {
				self.debug("No recipient or envelope address.").await?;
				rcpt.insert(self.recipients.get("_")
					.ok_or(MyError::NoDefault)?);
			};

			// prepating message header
			let mut reply: Vec<String> = vec![];
			if self.fields.contains("subject") {
				if let Some(subject) = mail.subject() {
					reply.push(format!("__*Subject:*__ `{}`", encode(subject)));
				} else if let Some(thread) = mail.thread_name() {
					reply.push(format!("__*Thread:*__ `{}`", encode(thread)));
				}
			}
			let mut short_headers: Vec<String> = vec![];
			// do we need to replace spaces here?
			if self.fields.contains("from") {
				short_headers.push(format!("__*From:*__ `{}`", encode(&headers.from)));
			}
			if self.fields.contains("date") {
				if let Some(date) = mail.date() {
					short_headers.push(format!("__*Date:*__ `{date}`"));
				}
			}
			reply.push(short_headers.join(" "));
			let header_size = reply.join(" ").len() + 1;

			let html_parts = mail.html_body_count();
			let text_parts = mail.text_body_count();
			let attachments = mail.attachment_count();
			if html_parts != text_parts {
				self.debug(&format!("Hm, we have {html_parts} HTML parts and {text_parts} text parts.")).await?;
			}
			//let mut html_num = 0;
			let mut text_num = 0;
			let mut file_num = 0;
			// let's display first html or text part as body
			let mut body = "".into();
			/*
			 * actually I don't wanna parse that html stuff
			if html_parts > 0 {
				let text = mail.body_html(0).unwrap();
				if text.len() < 4096 - header_size {
					body = text;
					html_num = 1;
				}
			};
			*/
			if body == "" && text_parts > 0 {
				let text = mail.body_text(0)
					.ok_or(MyError::NoText)?;
				if text.len() < 4096 - header_size {
					body = text;
					text_num = 1;
				}
			};
			reply.push("```".into());
			reply.extend(body.lines().map(|x| x.into()));
			reply.push("```".into());

			// and let's collect all other attachment parts
			let mut files_to_send = vec![];
			/*
			 * let's just skip html parts for now, they just duplicate text?
			while html_num < html_parts {
				files_to_send.push(mail.html_part(html_num).unwrap());
				html_num += 1;
			}
			*/
			while text_num < text_parts {
				files_to_send.push(mail.text_part(text_num)
					.ok_or(MyError::NoText)?);
				text_num += 1;
			}
			while file_num < attachments {
				files_to_send.push(mail.attachment(file_num)
					.ok_or(MyError::NoText)?);
				file_num += 1;
			}

			let msg = reply.join("\n");
			for chat in rcpt {
				if !files_to_send.is_empty() {
					let mut files = vec![];
					let mut first_one = true;
					for chunk in &files_to_send {
						let data = chunk.contents();
						let mut filename: Option<String> = None;
						for header in chunk.headers() {
							if header.name() == "Content-Type" {
								match header.value() {
									mail_parser::HeaderValue::ContentType(contenttype) => {
										if let Some(fname) = contenttype.attribute("name") {
											filename = Some(fname.to_owned());
										}
									},
									_ => {
										self.debug("Attachment has bad ContentType header.").await?;
									},
								};
							};
						};
						let filename = if let Some(fname) = filename {
							fname
						} else {
							"Attachment.txt".into()
						};
						let item = teloxide::types::InputMediaDocument::new(
							teloxide::types::InputFile::memory(data.to_vec())
							.file_name(filename));
						let item = if first_one {
							first_one = false;
							item.caption(&msg)
						} else {
							item
						};
						files.push(InputMedia::Document(item));
					}
					self.sendgroup(chat, files).await?;
				} else {
					self.send(chat, &msg).await?;
				}
			}
		} else {
			return Err(MyError::NoHeaders);
		}
		Ok(())
	}

	/// Send media to specified user
	pub async fn sendgroup <M> (&self, to: &ChatId, media: M) -> Result<Vec<Message>, MyError>
	where M: IntoIterator<Item = InputMedia> {
		Ok(self.tg.send_media_group(*to, media).await?)
	}
}

impl mailin_embedded::Handler for TelegramTransport {
	/// Just deny login auth
	fn auth_login (&mut self, _username: &str, _password: &str) -> Response {
		INVALID_CREDENTIALS
	}

	/// Just deny plain auth
	fn auth_plain (&mut self, _authorization_id: &str, _authentication_id: &str, _password: &str) -> Response {
		INVALID_CREDENTIALS
	}

	/// Verify whether address is deliverable
	fn rcpt (&mut self, to: &str) -> Response {
		if self.relay {
			OK
		} else {
			match self.recipients.get(to) {
				Some(_) => OK,
				None => {
					if self.relay {
						OK
					} else {
						NO_MAILBOX
					}
				}
			}
		}
	}

	/// Save headers we need
	fn data_start (&mut self, _domain: &str, from: &str, _is8bit: bool, to: &[String]) -> Response {
		self.headers = Some(SomeHeaders{
			from: from.to_string(),
			to: to.to_vec(),
		});
		OK
	}

	/// Save chunk(?) of data
	fn data (&mut self, buf: &[u8]) -> Result<(), Error> {
		self.data.append(buf.to_vec().as_mut());
		Ok(())
	}

	/// Attempt to send email, return temporary error if that fails
	fn data_end (&mut self) -> Response {
		let mut result = OK;
		task::block_on(async {
			// relay mail
			if let Err(err) = self.relay_mail().await {
				result = INTERNAL_ERROR;
				// in case that fails - inform default recipient
				if let Err(err) = self.debug(&format!("Sending emails failed:\n{err:?}")).await {
					// in case that also fails - write some logs and bail
					eprintln!("{err:?}");
				};
			};
		});
		// clear - just in case
		self.data = vec![];
		self.headers = None;
		result
	}
}

#[async_std::main]
async fn main () -> Result<()> {
	let specs = OptSpecs::new()
		.option("help", "h", OptValueType::None)
		.option("help", "help", OptValueType::None)
		.option("config", "c", OptValueType::Required)
		.option("config", "config", OptValueType::Required)
		.flag(OptFlags::OptionsEverywhere);
	let mut args = std::env::args();
	args.next();
	let parsed = specs.getopt(args);
	for u in &parsed.unknown {
		println!("Unknown option: {u}");
	}
	if !(parsed.unknown.is_empty()) || parsed.options_first("help").is_some() {
		println!("SMTP2TG v{}, (C) 2024 - 2025\n\n\
			\t-h|--help\tDisplay this help\n\
			\t-c|-config …\tSet configuration file location.",
			env!("CARGO_PKG_VERSION"));
		return Ok(());
	};
	let config_file = Path::new(if let Some(path) = parsed.options_value_last("config") {
		&path[..]
	} else {
		"smtp2tg.toml"
	});
	if !config_file.exists() {
		eprintln!("Error: can't read configuration from {config_file:?}");
		std::process::exit(1);
	};
	{
		let meta = metadata(config_file).await?;
		if (!0o100600 & meta.permissions().mode()) > 0 {
			eprintln!("Error: other users can read or write config file {config_file:?}\n\
				File permissions: {:o}", meta.permissions().mode());
			std::process::exit(1);
		}
	}
	let settings: config::Config = config::Config::builder()
		.set_default("fields", vec!["date", "from", "subject"]).unwrap()
		.set_default("hostname", "smtp.2.tg").unwrap()
		.set_default("listen_on", "0.0.0.0:1025").unwrap()
		.set_default("unknown", "relay").unwrap()
		.add_source(config::File::from(config_file))
		.build()
		.unwrap_or_else(|_| panic!("[{config_file:?}] there was an error reading config\n\
			\tplease consult \"smtp2tg.toml.example\" for details"));

	let listen_on = settings.get_string("listen_on")?;
	let server_name = settings.get_string("hostname")?;
	let core = TelegramTransport::new(settings);
	let mut server = mailin_embedded::Server::new(core);

	server.with_name(server_name)
		.with_ssl(mailin_embedded::SslConfig::None).unwrap()
		.with_addr(listen_on).unwrap();
	server.serve().unwrap();

	Ok(())
}
