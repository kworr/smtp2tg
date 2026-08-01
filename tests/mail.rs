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
fn get_id_properly_resolves_addresses () -> Result<()> {
	let server = build_server()?;
	let cases = [
		("someone@example.com", 1),
		("someone", 0),
		("root", -1),
		("unknown@example.com", 0),
		("SOMEONE@example.com", 1),	// uppercase local part
		("someone@EXAMPLE.COM", 1),	// uppercase domain
		("some.one@example.com", 1),	// functionally equivalent to skipping '.'
		("some-one-2", 0),	// Hyphens
	];
	for (email, id) in cases {
		assert_eq!(*server.get_id(email)?, ChatPeerId::from(id), "email [{email}] expected to return id [{id}]");
	}
	let cases = [
		"someone@otherdomain.net",
		"@example.com",             // empty local part
		"some@one@example.com",     // more than one '@'
		"someone@example.com.evil",
		"someone@example.org",
	];
	for email in cases {
		let err = server.get_id(email).err()
			.ok_or_else(|| format!("email [{email}] expected to fail")).stack()?;
		assert!(err.to_string().contains("Doesn't look like address from one of our domains."));
	}
	Ok(())
}
