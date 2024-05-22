use anyhow::Result;
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
use telegram_bot::{
	Api,
	ParseMode,
	SendMessage,
	UserId,
};

use std::{
	borrow::Cow,
	collections::{
		HashMap,
		HashSet,
	},
	io::Read,
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

fn relay_mails(maildir: &Path, core: &TelegramTransport) -> Result<()> {
	let new_dir = maildir.join("new");

	std::fs::create_dir_all(&new_dir)?;

	let files = std::fs::read_dir(new_dir)?;
	for file in files {
		let file = file?;
		let mut buf = Vec::new();
		std::fs::File::open(file.path())?.read_to_end(&mut buf)?;

		task::block_on(async move {
			match mail_parser::MessageParser::default().parse(&buf[..]) {
				Some(mail) => {
					let mail = mail.clone();

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
					let mut rcpt: HashSet<UserId> = HashSet::new();
					for item in to {
						let item = item.into_owned();
						if core.recipients.contains_key(&item) {
							rcpt.insert(core.recipients[&item]);
						} else {
							core.debug(format!("Recipient [{}] not found.", &item)).await.unwrap();
							rcpt.insert(core.default);
						}
					};
					if rcpt.is_empty() {
						rcpt.insert(core.default);
						core.debug("No recipient or envelope address.").await.unwrap();
					};

					// prepating message header
					let mut reply: Vec<Cow<str>> = vec![];
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
						core.debug(format!("Hm, we have {} HTML parts and {} text parts.", html_parts, text_parts)).await.unwrap();
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
						let text = mail.body_text(0).unwrap();
						if text.len() < 4096 - header_size {
							body = text;
							text_num = 1;
						}
					};
					reply.push("```".into());
					for line in body.lines() {
						reply.push(line.into());
					}
					reply.push("```".into());

					// and let's coillect all other attachment parts
					let mut files_to_send = vec![];
					/*
					 * let's just skip html parts for now, they just duplicate text?
					while html_num < html_parts {
						files_to_send.push(mail.html_part(html_num).unwrap());
						html_num += 1;
					}
					*/
					while text_num < text_parts {
						files_to_send.push(mail.text_part(text_num).unwrap());
						text_num += 1;
					}
					while file_num < attachments {
						files_to_send.push(mail.attachment(file_num).unwrap());
						file_num += 1;
					}

					for chat in rcpt {
						core.send(chat, reply.join("\n")).await.unwrap();
						for chunk in &files_to_send {
							let data = chunk.contents().to_vec();
							let obj = telegram_bot::types::InputFileUpload::with_data(data, "Attachment");
							core.sendfile(chat, obj).await.unwrap();
						}
					}
				},
				None => { core.debug("None mail.").await.unwrap(); },
				//send_to_sendgrid(mail, sendgrid_api_key).await;
			};
		});

		std::fs::remove_file(file.path())?;
	}
	Ok(())
}

fn my_prudence() -> Prudence {
	Prudence::default().with_read_timeout(Duration::from_secs(60)).with_banner_delay(Duration::from_secs(1))
}

pub struct TelegramTransport {
	default: UserId,
	tg: Api,
	recipients: HashMap<String, UserId>,
}

impl TelegramTransport {
	pub fn new(settings: &config::Config) -> TelegramTransport {
		let api_key = settings.get_string("api_key").unwrap();
		let tg = Api::new(api_key);
		let default_recipient = settings.get_string("default").unwrap();
		let recipients: HashMap<String, UserId> = settings.get_table("recipients").unwrap().into_iter().map(|(a, b)| (a, UserId::new(b.into_int().unwrap()))).collect();
		// Barf if no default
		let default = recipients[&default_recipient];

		TelegramTransport {
			default,
			tg,
			recipients,
		}
	}

	pub async fn debug<'b, S>(&self, msg: S) -> Result<()>
	where S: Into<Cow<'b, str>> {
		task::sleep(Duration::from_secs(5)).await;
		self.tg.send(SendMessage::new(self.default, msg)
			.parse_mode(ParseMode::Markdown)).await?;
		Ok(())
	}

	pub async fn send<'b, S>(&self, to: UserId, msg: S) -> Result<()>
	where S: Into<Cow<'b, str>> {
		task::sleep(Duration::from_secs(5)).await;
		self.tg.send(SendMessage::new(to, msg)
			.parse_mode(ParseMode::Markdown)).await?;
		Ok(())
	}

	pub async fn sendfile<V>(&self, to: UserId, chunk: V) -> Result<()>
	where V: Into<telegram_bot::InputFile> {
		task::sleep(Duration::from_secs(5)).await;
		self.tg.send(telegram_bot::SendDocument::new(to, chunk)).await?;
		Ok(())
	}
}

#[async_std::main]
async fn main() {
	let settings: config::Config = config::Config::builder()
		.add_source(config::File::with_name("smtp2tg.toml"))
		.build().unwrap();

	let core = TelegramTransport::new(&settings);
	let maildir: PathBuf = settings.get_string("maildir").unwrap().into();
	let listen_on = settings.get_string("listen_on").unwrap();
	let sink = Builder + Name::new("smtp2tg") + DebugService +
		my_prudence() + MailDir::new(maildir.clone()).unwrap();

	task::spawn(async move {
		loop {
			relay_mails(&maildir, &core).unwrap();
			task::sleep(Duration::from_secs(5)).await;
		}
	});

	match listen_on.as_str() {
		"socket" => {
			let sink = sink + samotop::smtp::Lmtp.with(SmtpParser);
			samotop::server::UnixServer::on("./smtp2tg.sock")
				.serve(sink.build()).await.unwrap();
		},
		_ => {
			let sink = sink + samotop::smtp::Esmtp.with(SmtpParser);
			samotop::server::TcpServer::on(listen_on)
				.serve(sink.build()).await.unwrap();
		},
	};
}
