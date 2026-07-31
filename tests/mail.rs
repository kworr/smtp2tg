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
fn get_id_returns_configured_recipient () -> Result<()> {
	let server = build_server()?;
	let cases = [
		("someone@example.com", 1),
		("someone", 0),
		("root", -1),
		("unknown@example.com", 0),
	];
	for (email, id) in cases {
		assert_eq!(*server.get_id(email)?, ChatPeerId::from(id), "email [{email}] expected to return id [{id}]");
	}
	Ok(())
}
