use crate::utils::Attachment;

use std::{
	collections::HashMap,
	fmt::Debug,
};

use stacked_errors::{
	Result,
	StackableErr,
};
use tgbot::{
	api::Client,
	types::{
		ChatPeerId,
		InputFile,
		InputFileReader,
		InputMediaDocument,
		MediaGroup,
		MediaGroupItem,
		Message,
		ParseMode::Html,
		SendMediaGroup,
		SendMessage,
		SendDocument,
	},
};

#[derive(Debug)]
pub struct TelegramTransport {
	tg: Client,
	recipients: HashMap<String, ChatPeerId>,
	pub default: ChatPeerId,
}

impl TelegramTransport {
	/// Creates new TelegramTransport object.
	pub fn new (api_key: String, recipients: HashMap<String, i64>, settings: &config::Config) -> Result<TelegramTransport> {
		let default = settings.get_int("default")
			.context("[smtp2tg.toml] missing \"default\" recipient.\n")?;
		let api_gateway = settings.get_string("api_gateway")
			.context("[smtp2tg.toml] missing \"api_gateway\" destination.\n")?;

		let tg = Client::new(api_key)
			.context("Failed to create API.\n")?
			.with_host(api_gateway);
		let recipients = recipients.into_iter()
			.map(|(a, b)| (a, ChatPeerId::from(b))).collect();
		let default = ChatPeerId::from(default);

		Ok(TelegramTransport {
			tg,
			recipients,
			default,
		})
	}

	/// Send message to default user, used for debug/log/info purposes
	pub async fn debug (&self, msg: &str) -> Result<Message> {
		self.send(&self.default, format!("<pre>{msg}</pre>")).await
	}

	/// Get recipient by address
	pub fn get (&self, name: &str) -> Result<&ChatPeerId> {
		self.recipients.get(name)
			.with_context(|| format!("Recipient \"{name}\" not found in configuration"))
	}

	/// Send message to specified user
	pub async fn send <S> (&self, to: &ChatPeerId, msg: S) -> Result<Message>
	where S: Into<String> + Debug{
		self.tg.execute(
			SendMessage::new(*to, msg)
			.with_parse_mode(Html)
		).await.stack()
	}

	/// Send media to specified user
	pub async fn sendgroup (&self, to: &ChatPeerId, media: Vec<Attachment>, msg: &str) -> Result<()> {
		if media.len() > 1 {
			let mut attach = vec![];
			let mut pos = media.len();
			for file in media {
				let mut caption = InputMediaDocument::default();
				if pos == 1 {
					caption = caption.with_caption(msg)
						.with_caption_parse_mode(Html);
				}
				pos -= 1;
				attach.push(
					MediaGroupItem::for_document(
						InputFile::from(
							InputFileReader::from(file.data)
								.with_file_name(file.name)
						),
						caption
					)
				);
			}
			self.tg.execute(SendMediaGroup::new(*to, MediaGroup::new(attach).stack()?)).await.stack()?;
		} else {
			self.tg.execute(
				SendDocument::new(
					*to,
					InputFileReader::from(media[0].data.clone())
					.with_file_name(media[0].name.clone())
				).with_caption(msg)
				.with_caption_parse_mode(Html)
			).await.stack()?;
		}
		Ok(())
	}
}
