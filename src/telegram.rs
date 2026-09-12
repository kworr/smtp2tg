//! Telegram API integration for sending messages and attachments.

use crate::utils::{
	Attachment,
	validate,
};

use std::{
	collections::HashMap,
	fmt::Debug,
};

use stacked_errors::{
	bail,
	Result,
	StackableErr,
};
use tgbot::{
	api::Client,
	types::{
		ChatPeerId, InputFile, InputFileReader, InputMediaDocument, InputText, MediaGroup, MediaGroupItem, Message, ParseMode::Html, SendDocument, SendMediaGroup, SendMessage
	},
};

#[derive(Debug)]
pub struct TelegramTransport {
	tg: Client,
	recipients: HashMap<String, ChatPeerId>,
	pub default: ChatPeerId,
}

impl TelegramTransport {
	/// Creates a new `TelegramTransport` instance.
	///
	/// # Arguments
	/// * `api_key` - Telegram Bot API token.
	/// * `recipients` - Mapping of email addresses to Telegram chat IDs.
	/// * `settings` - Additional configuration (API gateway, default chat).
	///
	/// # Errors
	/// Returns an error if configuration values cannot be read or if Telegram
	/// API client creation fails.
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

	/// Sends a debug message to the default chat.
	///
	/// # Arguments
	/// * `msg` - Message text to send.
	///
	/// # Returns
	/// * `Result<Message>` - Telegram API response.
	///
	/// # Errors
	/// Returns an error if `msg` contains a closing Telegram tag or sending fails.
	pub async fn debug (&self, msg: &str) -> Result<Message> {
		self.send(&self.default, &format!("<pre>{}</pre>", validate(msg).stack()?)).await
	}

	/// Retrieves a chat ID by name.
	///
	/// # Arguments
	/// * `name` - Name or email to look up.
	///
	/// # Returns
	/// * `Result<&ChatPeerId>` - Chat ID if found.
	///
	/// # Errors
	/// Returns an error if `name` is not configured.
	pub fn get (&self, name: &str) -> Result<&ChatPeerId> {
		self.recipients.get(&name.to_lowercase())
			.with_context(|| format!("Recipient {name:?} not found in configuration"))
	}

	/// Sends a text message to a specified chat.
	///
	/// # Arguments
	/// * `to` - Target chat ID.
	/// * `msg` - Message text (supports HTML formatting).
	///
	/// # Returns
	/// * `Result<Message>` - Telegram API response.
	pub async fn send (&self, to: &ChatPeerId, msg: &str) -> Result<Message> {
		self.tg.execute(
			SendMessage::new(*to, InputText::from(msg).with_format(Html))
		).await.stack()
	}

	/// Sends a message with attachments to a specified chat.
	///
	/// # Arguments
	/// * `to` - Target chat ID.
	/// * `media` - List of attachments, non-empty.
	/// * `msg` - Message text (supports HTML formatting).
	///
	/// # Returns
	/// * `Result<()>` - fails if `media` is empty, problems forming media
	/// group, request fails.
	pub async fn sendgroup (&self, to: &ChatPeerId, media: Vec<Attachment>, msg: &str) -> Result<()> {
		if media.len() > 1 {
			let mut attach = vec![];
			let mut pos = media.len();
			for file in media {
				let mut doc = InputMediaDocument::from(
					InputFile::from(
						InputFileReader::from(file.data)
							.with_file_name(file.name)
					)
				);
				if pos == 1 {
					doc = doc.with_caption(InputText::from(msg)
						.with_format(Html));
				}
				pos -= 1;
				attach.push(MediaGroupItem::from(doc));
			}
			self.tg.execute(SendMediaGroup::new(*to, MediaGroup::new(attach).stack()?)).await.stack()?;
		} else {
			if media.is_empty() {
				bail!("At least one attachment is required.");
			}
			self.tg.execute(
				SendDocument::new(
					*to,
					InputFileReader::from(media[0].data.clone())
						.with_file_name(media[0].name.clone())
				).with_caption(InputText::from(msg)
					.with_format(Html))
			).await.stack()?;
		}
		Ok(())
	}
}
