use anyhow::Result;
use async_std::task;
//use async_trait::async_trait;
//use futures::io::AsyncRead;
//use mail_parser::Message;
use samotop::{
	mail::{
		Builder,
		DebugService,
		MailDir,
		Name
	},
	smtp::Prudence,
};
use telegram_bot::{
	Api,
	ParseMode,
	SendMessage,
	UserId,
};

use std::{
	borrow::Cow,
	collections::HashMap,
	io::Read,
	path::{
		Path,
		PathBuf
	},
	time::Duration,
	vec::Vec,
};


fn relay_mails(maildir: &Path, core: &Core) -> Result<()> {
	use mail_parser::*;

	let new_dir = maildir.join("new");

	std::fs::create_dir_all(&new_dir)?;

	let files = std::fs::read_dir(new_dir)?;
	for file in files {
		dbg!(&file);
		let file = file?;
		let mut buf = Vec::new();
		std::fs::File::open(file.path())?.read_to_end(&mut buf)?;

		task::block_on(async move {
			match MessageParser::default().parse(&buf[..]) {
				Some(mail) => {
					/*
					dbg!(&mail);
					let to = match mail.to() {
						Some(mail) => mail.into_list().into_iter().map(|a| a.address.unwrap()).collect(),
						None => match mail.header("X-Samotop-To").unwrap() {
							mail_parser::HeaderValue::Address(addr) => addr.address.unwrap(),
						},
					};
					dbg!(&to);
					*/
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

pub struct Core {
	default: UserId,
	tg: Api,
	recipients: HashMap<String, UserId>,
}

impl Core {
	pub fn new(settings: &config::Config) -> Result<Core> {
		let api_key = settings.get_string("api_key").unwrap();
		let tg = Api::new(api_key);
		let default_recipient = settings.get_string("default")?;
		let recipients: HashMap<String, UserId> = settings.get_table("recipients")?.into_iter().map(|(a, b)| (a, UserId::new(b.into_int().unwrap()))).collect();
		let default = recipients[&default_recipient];

		Ok(Core {
			default,
			tg,
			recipients,
		})
	}

	pub async fn debug<'b, S>(&self, msg: S) -> Result<()>
	where S: Into<Cow<'b, str>> {
		self.tg.send(SendMessage::new(self.default, msg)
			.parse_mode(ParseMode::Markdown)).await?;
		Ok(())
	}

	pub async fn send<'b, S>(&self, to: String, msg: S) -> Result<()>
	where S: Into<Cow<'b, str>> {
		self.tg.send(SendMessage::new(self.recipients[&to], msg)
			.parse_mode(ParseMode::Markdown)).await?;
		Ok(())
	}
}

#[async_std::main]
async fn main() {
	let settings: config::Config = config::Config::builder()
		.add_source(config::File::with_name("smtp2tg.toml"))
		.build().unwrap();

	let core = Core::new(&settings).unwrap();
	let maildir: PathBuf = settings.get_string("maildir").unwrap().into();
	let addr = "./smtp2tg.sock";
	let listen_on = settings.get_string("listen_on").unwrap();
	let sink = Builder + Name::new("smtp2tg") + DebugService +
		samotop::smtp::Esmtp.with(samotop::smtp::SmtpParser) + my_prudence() +
		MailDir::new(maildir.clone()).unwrap();

	task::spawn(async move {
		loop {
			task::sleep(Duration::from_secs(5)).await;
			relay_mails(&maildir, &core).unwrap();
		}
	});

	match listen_on.as_str() {
		"socket" => samotop::server::UnixServer::on("./smtp2tg.sock")
			.serve(sink.build()).await.unwrap(),
		_ => samotop::server::TcpServer::on(listen_on)
			.serve(sink.build()).await.unwrap(),
	};
	/*
	task::block_on(async {
		let be = MyBackend;

		//let mut s = Server::new(be);

		s.addr = "127.0.0.1:2525".to_string();
		s.domain = "localhost".to_string();
		s.read_timeout = std::time::Duration::from_secs(10);
		s.write_timeout = std::time::Duration::from_secs(10);
		s.max_message_bytes = 10 * 1024 * 1024;
		s.max_recipients = 50;
		s.max_line_length = 1000;
		s.allow_insecure_auth = true;

		println!("Starting server on {}", s.addr);
		match s.listen_and_serve().await {
			Ok(_) => println!("Server stopped"),
			Err(e) => println!("Server error: {}", e),
		}
		Ok(())
	})
	*/
}
