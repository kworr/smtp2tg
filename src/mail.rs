use crate::{
	Cursor,
	telegram::{
		encode,
		TelegramTransport,
	},
	utils::{
		Attachment,
		RE_DOMAIN,
	},
};

use std::{
	borrow::Cow,
	collections::{
		HashMap,
		HashSet,
	},
	io::Error,
};

use anyhow::{
	bail,
	Context,
	Result,
};
use async_std::{
	sync::Arc,
	task,
};
use mailin_embedded::{
	Response,
	response::{
		INTERNAL_ERROR,
		INVALID_CREDENTIALS,
		NO_MAILBOX,
		OK
	},
};
use regex::{
	Regex,
	escape,
};
use tgbot::types::ChatPeerId;

/// `SomeHeaders` object to store data through SMTP session
#[derive(Clone, Debug)]
struct SomeHeaders {
	from: String,
	to: Vec<String>,
}

/// `MailServer` Central object with TG api and configuration
#[derive(Clone, Debug)]
pub struct MailServer {
	data: Vec<u8>,
	headers: Option<SomeHeaders>,
	relay: bool,
	tg: Arc<TelegramTransport>,
	fields: HashSet<String>,
	address: Regex,
}

impl MailServer {
	/// Initialize API and read configuration
	pub fn new(settings: config::Config) -> Result<MailServer> {
		let api_key = settings.get_string("api_key")
			.context("[smtp2tg.toml] missing \"api_key\" parameter.\n")?;
		let mut recipients = HashMap::new();
		for (name, value) in settings.get_table("recipients")
			.expect("[smtp2tg.toml] missing table \"recipients\".\n")
		{
			let value = value.into_int()
				.context("[smtp2tg.toml] \"recipient\" table values should be integers.\n")?;
			recipients.insert(name, value);
		}
		let default = settings.get_int("default")
			.context("[smtp2tg.toml] missing \"default\" recipient.\n")?;

		let tg = Arc::new(TelegramTransport::new(api_key, recipients, default)?);
		let fields = HashSet::<String>::from_iter(settings.get_array("fields")
			.expect("[smtp2tg.toml] \"fields\" should be an array")
			.iter().map(|x| x.clone().into_string().expect("should be strings")));
		let mut domains: HashSet<String> = HashSet::new();
		let extra_domains = settings.get_array("domains").unwrap();
		for domain in extra_domains {
			let domain = domain.to_string().to_lowercase();
			if RE_DOMAIN.is_match(&domain) {
				domains.insert(domain);
			} else {
				panic!("[smtp2tg.toml] can't check of domains in \"domains\": {domain}");
			}
		}
		let domains = domains.into_iter().map(|s| escape(&s))
			.collect::<Vec<String>>().join("|");
		let address = Regex::new(&format!("^(?P<user>[a-z0-9][-a-z0-9])(@({domains}))$")).unwrap();
		let relay = match settings.get_string("unknown")
			.context("[smtp2tg.toml] can't get \"unknown\" policy.\n")?.as_str()
		{
			"relay" => true,
			"deny" => false,
			_ => {
				bail!("[smtp2tg.toml] \"unknown\" should be either \"relay\" or \"deny\".\n");
			},
		};

		Ok(MailServer {
			data: vec!(),
			headers: None,
			relay,
			tg,
			fields,
			address,
		})
	}

	/// Returns id for provided email address
	fn get_id (&self, name: &str) -> Result<&ChatPeerId> {
		// here we need to store String locally to borrow it after
		let mut link = name;
		let name: String;
		if let Some(caps) = self.address.captures(link) {
			name = caps["name"].to_string();
			link = &name;
		}
		match self.tg.get(link) {
			Ok(addr) => Ok(addr),
			Err(_) => Ok(&self.tg.default),
		}
	}

	/// Attempt to deliver one message
	async fn relay_mail (&self) -> Result<()> {
		if let Some(headers) = &self.headers {
			let mail = mail_parser::MessageParser::new().parse(&self.data)
				.context("Failed to parse mail.")?;

			// Adding all known addresses to recipient list, for anyone else adding default
			// Also if list is empty also adding default
			let mut rcpt: HashSet<&ChatPeerId> = HashSet::new();
			if headers.to.is_empty() && !self.relay {
				bail!("Relaying is disabled, and there's no destination address");
			}
			for item in &headers.to {
				rcpt.insert(self.get_id(item)?);
			};
			if rcpt.is_empty() {
				self.tg.debug("No recipient or envelope address.").await?;
				rcpt.insert(&self.tg.default);
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
				self.tg.debug(&format!("Hm, we have {html_parts} HTML parts and {text_parts} text parts.")).await?;
			}
			//let mut html_num = 0;
			let mut text_num = 0;
			let mut file_num = 0;
			// let's display first html or text part as body
			let mut body: Cow<'_, str> = "".into();
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
			if body.is_empty() && text_parts > 0 {
				let text = mail.body_text(0)
					.context("Failed to extract text from message")?;
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
				files_to_send.push(mail.text_part(text_num.try_into()?)
					.context("Failed to get text part from message.")?);
				text_num += 1;
			}
			while file_num < attachments {
				files_to_send.push(mail.attachment(file_num.try_into()?)
					.context("Failed to get file part from message.")?);
				file_num += 1;
			}

			let msg = reply.join("\n");
			for chat in rcpt {
				if !files_to_send.is_empty() {
					let mut files = vec![];
					// let mut first_one = true;
					for chunk in &files_to_send {
						let data: Vec<u8> = chunk.contents().to_vec();
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
										self.tg.debug("Attachment has bad ContentType header.").await?;
									},
								};
							};
						};
						let filename = if let Some(fname) = filename {
							fname
						} else {
							"Attachment.txt".into()
						};
						files.push(Attachment {
							data: Cursor::new(data),
							name: filename,
						});
					}
					self.tg.sendgroup(chat, files, &msg).await?;
				} else {
					self.tg.send(chat, &msg).await?;
				}
			}
		} else {
			bail!("Required headers were not found.");
		}
		Ok(())
	}
}

impl mailin_embedded::Handler for MailServer {
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
			match self.get_id(to) {
				Ok(_) => OK,
				Err(_) => {
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
				if let Err(err) = self.tg.debug(&format!("Sending emails failed:\n{err:?}")).await {
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
