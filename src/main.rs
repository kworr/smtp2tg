//! Simple SMTP-to-Telegram gateway. Can parse email and send them as telegram
//! messages to specified chats, generally you specify which email address is
//! available in configuration, everything else is sent to default address.

use anyhow::{
	anyhow,
	bail,
	Result,
};
use async_std::{
	io::Error,
	task,
};
use mailin_embedded::{
	Response,
	response::*,
};
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

use std::{
	borrow::Cow,
	collections::{
		HashMap,
		HashSet,
	},
	vec::Vec,
};

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
				eprintln!("[smtp2tg.toml] can't get \"unknown\":\n {}\n", err);
				panic!("bad setting");
			},
		};

		TelegramTransport {
			data: vec!(),
			headers: None,
			recipients,
			relay,
			tg,
		}
	}

	/// Send message to default user, used for debug/log/info purposes
	async fn debug<'b, S>(&self, msg: S) -> Result<Message>
	where S: Into<String> {
		Ok(self.tg.send_message(*self.recipients.get("_").unwrap(), msg).await?)
	}

	/// Send message to specified user
	async fn send<'b, S>(&self, to: &ChatId, msg: S) -> Result<Message>
	where S: Into<String> {
		Ok(self.tg.send_message(*to, msg).await?)
	}

	/// Attempt to deliver one message
	async fn relay_mail (&self) -> Result<()> {
		if let Some(headers) = &self.headers {
			let mail = mail_parser::MessageParser::new().parse(&self.data)
				.ok_or(anyhow!("Failed to parse mail"))?;

			// Adding all known addresses to recipient list, for anyone else adding default
			// Also if list is empty also adding default
			let mut rcpt: HashSet<&ChatId> = HashSet::new();
			if headers.to.is_empty() {
				bail!("No recipient addresses.");
			}
			for item in &headers.to {
				match self.recipients.get(item) {
					Some(addr) => rcpt.insert(addr),
					None => {
						self.debug(format!("Recipient [{}] not found.", &item)).await?;
						rcpt.insert(self.recipients.get("_")
							.ok_or(anyhow!("Missing default address in recipient table."))?)
					}
				};
			};
			if rcpt.is_empty() {
				self.debug("No recipient or envelope address.").await?;
				rcpt.insert(self.recipients.get("_")
					.ok_or(anyhow!("Missing default address in recipient table."))?);
			};

			// prepating message header
			let mut reply: Vec<Cow<'_, str>> = vec![];
			if let Some(subject) = mail.subject() {
				reply.push(format!("**Subject:** `{}`", subject).into());
			} else if let Some(thread) = mail.thread_name() {
				reply.push(format!("**Thread:** `{}`", thread).into());
			}
			reply.push(format!("**From:** `{}`", headers.from).into());
			reply.push("".into());
			let header_size = reply.join("\n").len() + 1;

			let html_parts = mail.html_body_count();
			let text_parts = mail.text_body_count();
			let attachments = mail.attachment_count();
			if html_parts != text_parts {
				self.debug(format!("Hm, we have {} HTML parts and {} text parts.", html_parts, text_parts)).await?;
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
					.ok_or(anyhow!("Failed to extract text from message."))?;
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
					.ok_or(anyhow!("Failed to get text part from message"))?);
				text_num += 1;
			}
			while file_num < attachments {
				files_to_send.push(mail.attachment(file_num)
					.ok_or(anyhow!("Failed to get file part from message"))?);
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
							item.caption(&msg).parse_mode(MarkdownV2)
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
			bail!("No headers.");
		}
		Ok(())
	}

	/// Send media to specified user
	pub async fn sendgroup<M>(&self, to: &ChatId, media: M) -> Result<Vec<Message>>
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
	fn data(&mut self, buf: &[u8]) -> Result<(), Error> {
		self.data.append(buf.to_vec().as_mut());
		Ok(())
	}

	/// Attempt to send email, return temporary error if that fails
	fn data_end(&mut self) -> Response {
		let mut result = OK;
		task::block_on(async {
			// relay mail
			if let Err(err) = self.relay_mail().await {
				result = INTERNAL_ERROR;
				// in case that fails - inform default recipient
				if let Err(err) = self.debug(format!("Sending emails failed:\n{:?}", err)).await {
					// in case that also fails - write some logs and bail
					eprintln!("Failed to contact Telegram:\n{:?}", err);
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
async fn main() -> Result<()> {
	let settings: config::Config = config::Config::builder()
		.set_default("listen_on", "0.0.0.0:1025").unwrap()
		.set_default("hostname", "smtp.2.tg").unwrap()
		.set_default("unknown", "relay").unwrap()
		.add_source(config::File::with_name("smtp2tg.toml"))
		.build()
		.expect("[smtp2tg.toml] there was an error reading config\n\
			\tplease consult \"smtp2tg.toml.example\" for details");

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
