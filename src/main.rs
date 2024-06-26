use anyhow::{
	anyhow,
	Result,
};
use async_std::task;
use samotop::{
	mail::{
		Builder,
		DebugService,
		MailDir,
		Name
	},
	smtp::{
		SmtpParser,
		Prudence,
	},
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
	io::Read,
	os::unix::fs::{
		FileTypeExt,
		PermissionsExt,
	},
	path::{
		Path,
		PathBuf
	},
	time::Duration,
	vec::Vec,
};

fn address_into_iter<'a>(addr: &'a mail_parser::Address<'a, >) -> impl Iterator<Item = Cow<'a, str>> {
	addr.clone().into_list().into_iter().map(|a| a.address.unwrap())
}

async fn relay_mails(maildir: &Path, core: &TelegramTransport) -> Result<()> {
	let new_dir = maildir.join("new");

	std::fs::create_dir_all(&new_dir)?;

	let files = std::fs::read_dir(new_dir)?;
	for file in files {
		let file = file?;
		let mut buf: String = Default::default();
		std::fs::File::open(file.path())?.read_to_string(&mut buf)?;

		let mail = mail_parser::MessageParser::new().parse(&buf)
			.ok_or(anyhow!("Failed to parse mail `{:?}`.", file))?;

		// Fetching address lists from fields we know
		let mut to = HashSet::new();
		if let Some(addr) = mail.to() {
			let _ = address_into_iter(addr).map(|x| to.insert(x));
		};
		if let Some(addr) = mail.header("X-Samotop-To") {
			match addr {
				mail_parser::HeaderValue::Address(addr) => {
					let _ = address_into_iter(addr).map(|x| to.insert(x));
				},
				mail_parser::HeaderValue::Text(text) => {
					to.insert(text.clone());
				},
				_ => {}
			}
		};

		// Adding all known addresses to recipient list, for anyone else adding default
		// Also if list is empty also adding default
		let mut rcpt: HashSet<&ChatId> = HashSet::new();
		for item in to {
			let item = item.into_owned();
			match core.recipients.get(&item) {
				Some(addr) => rcpt.insert(addr),
				None => {
					core.debug(format!("Recipient [{}] not found.", &item)).await?;
					rcpt.insert(core.recipients.get("_")
						.ok_or(anyhow!("Missing default address in recipient table."))?)
				}
			};
		};
		if rcpt.is_empty() {
			core.debug("No recipient or envelope address.").await?;
			rcpt.insert(core.recipients.get("_")
				.ok_or(anyhow!("Missing default address in recipient table."))?);
		};

		// prepating message header
		let mut reply: Vec<Cow<'_, str>> = vec![];
		if let Some(subject) = mail.subject() {
			reply.push(format!("**Subject:** `{}`", subject).into());
		} else if let Some(thread) = mail.thread_name() {
			reply.push(format!("**Thread:** `{}`", thread).into());
		}
		if let Some(from) = mail.from() {
			reply.push(format!("**From:** `{:?}`", address_into_iter(from).collect::<Vec<_>>().join(", ")).into());
		}
		if let Some(sender) = mail.sender() {
			reply.push(format!("**Sender:** `{:?}`", address_into_iter(sender).collect::<Vec<_>>().join(", ")).into());
		}
		reply.push("".into());
		let header_size = reply.join("\n").len() + 1;

		let html_parts = mail.html_body_count();
		let text_parts = mail.text_body_count();
		let attachments = mail.attachment_count();
		if html_parts != text_parts {
			core.debug(format!("Hm, we have {} HTML parts and {} text parts.", html_parts, text_parts)).await?;
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
									core.debug("Attachment has bad ContentType header.").await?;
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
				core.sendgroup(chat, files).await?;
			} else {
				core.send(chat, &msg).await?;
			}
		}

		std::fs::remove_file(file.path())?;
	}
	Ok(())
}

fn my_prudence() -> Prudence {
	Prudence::default().with_read_timeout(Duration::from_secs(60)).with_banner_delay(Duration::from_secs(1))
}

pub struct TelegramTransport {
	tg: teloxide::adaptors::DefaultParseMode<teloxide::adaptors::Throttle<Bot>>,
	recipients: HashMap<String, ChatId>,
}

impl TelegramTransport {
	pub fn new(settings: config::Config) -> TelegramTransport {
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

		TelegramTransport {
			tg,
			recipients,
		}
	}

	pub async fn debug<'b, S>(&self, msg: S) -> Result<Message>
	where S: Into<String> {
		Ok(self.tg.send_message(*self.recipients.get("_").unwrap(), msg).await?)
	}

	pub async fn send<'b, S>(&self, to: &ChatId, msg: S) -> Result<Message>
	where S: Into<String> {
		Ok(self.tg.send_message(*to, msg).await?)
	}

	pub async fn sendgroup<M>(&self, to: &ChatId, media: M) -> Result<Vec<Message>>
	where M: IntoIterator<Item = InputMedia> {
		Ok(self.tg.send_media_group(*to, media).await?)
	}
}

#[async_std::main]
async fn main() {
	let settings: config::Config = config::Config::builder()
		.add_source(config::File::with_name("smtp2tg.toml"))
		.build()
		.expect("[smtp2tg.toml] there was an error reading config\n\
			\tplease consult \"smtp2tg.toml.example\" for details");

	let maildir: PathBuf = settings.get_string("maildir")
		.expect("[smtp2tg.toml] missing \"maildir\" parameter.\n").into();
	let listen_on = settings.get_string("listen_on")
		.expect("[smtp2tg.toml] missing \"listen_on\" parameter.\n");
	let core = TelegramTransport::new(settings);
	let sink = Builder + Name::new("smtp2tg") + DebugService +
		my_prudence() + MailDir::new(maildir.clone()).unwrap();

	task::spawn(async move {
		loop {
			// relay mails
			if let Err(err) = relay_mails(&maildir, &core).await {
				// in case that fails - inform default recipient
				if let Err(err) = core.debug(format!("Sending emails failed:\n{:?}", err)).await {
					// in case that also fails - write some logs and bail
					eprintln!("Failed to contact Telegram:\n{:?}", err);
				};
				task::sleep(Duration::from_secs(5 * 60)).await;
			};
			task::sleep(Duration::from_secs(5)).await;
		}
	});

	match listen_on.as_str() {
		"socket" => {
			let socket_path = "./smtp2tg.sock";
			match std::fs::symlink_metadata(socket_path) {
				Ok(metadata) => {
					if metadata.file_type().is_socket() {
						std::fs::remove_file(socket_path)
							.expect("[smtp2tg] failed to remove old socket.\n");
					} else {
						eprintln!("[smtp2tg] \"./smtp2tg.sock\" we wanted to use is actually not a socket.\n\
							[smtp2tg] please check the file and remove it manually.\n");
						panic!("socket path unavailable");
					}
				},
				Err(err) => {
					match err.kind() {
						std::io::ErrorKind::NotFound => {},
						_ => {
							eprintln!("{:?}", err);
							panic!("unhandled file type error");
						}
					};
				}
			};

			let sink = sink + samotop::smtp::Lmtp.with(SmtpParser);
			task::spawn(async move {
				// Postpone mode change on the socket. I can't actually change
				// other way, as UnixServer just grabs path, and blocks
				task::sleep(Duration::from_secs(1)).await;
				std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o777)).unwrap();
			});
			samotop::server::UnixServer::on(socket_path)
				.serve(sink.build()).await.unwrap();
		},
		_ => {
			let sink = sink + samotop::smtp::Esmtp.with(SmtpParser);
			samotop::server::TcpServer::on(listen_on)
				.serve(sink.build()).await.unwrap();
		},
	};
}
