use smtp2tg::mail::MailServer;

use stacked_errors::{
	Result,
	StackableErr,
};

use tgbot::types::ChatPeerId;
/// Builds a `MailServer` purely from an in-memory TOML source, no
/// network access is performed while constructing it.
fn build_server () -> Result<MailServer> {
	let settings = config::Config::builder()
		.add_source(config::File::from_str(r#"
			api_key = "test-api-key"
			api_gateway = "https://api.telegram.org"
			default = 0
			unknown = "relay"
			fields = ["date", "from", "subject"]
			domains = ["example.com"]

			[recipients]
			"someone@example.com" = 1
			"root" = -1
		"#, config::FileFormat::Toml))
		.build()
		.stack()?;
	MailServer::new(settings)
}

#[test]
fn get_id_returns_configured_recipient_for_full_address () -> Result<()> {
	let server = build_server()?;
	assert_eq!(*server.get_id("someone@example.com")?, ChatPeerId::from(1));
	Ok(())
}

#[test]
fn get_id_returns_configured_recipient_for_bare_name () -> Result<()> {
	let server = build_server()?;
	assert_eq!(*server.get_id("root")?, ChatPeerId::from(-1));
	Ok(())
}

#[test]
fn get_id_falls_back_to_default_for_unknown_address () -> Result<()> {
	let server = build_server()?;
	assert_eq!(*server.get_id("unknown@example.com")?, ChatPeerId::from(0));
	Ok(())
}
