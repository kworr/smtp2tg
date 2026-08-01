use smtp2tg::mail::MailServer;

use config::FileFormat::Toml;
use stacked_errors::{
	Result,
	StackableErr,
	ensure,
	ensure_eq,
};

use tgbot::types::ChatPeerId;

#[test]
fn get_id_properly_resolves_addresses () -> Result<()> {
	let server = MailServer::new(config::Config::builder()
		.add_source(config::File::from_str(r#"
			api_key = "test-api-key"
			api_gateway = "https://api.telegram.org"
			default = 0
			fields = ["date", "from", "subject"]
			domains = ["example.com"]

			[recipients]
			"someone@example.com" = 1
			"root" = -1
		"#, Toml))
		.build()
		.stack()?)?;
	let cases = [
		("someone@example.com", 1),
		("someone", 0),
		("root", -1),
		("unknown@example.com", 0),
		("SOMEONE@example.com", 1),	// uppercase local part
		("someone@EXAMPLE.COM", 1),	// uppercase domain
		("some.one@example.com", 0),	// dot are not skipped
		("some-one-2", 0),	// Hyphens
	];
	for (email, id) in cases {
		ensure_eq!(*server.get_id(email)?, ChatPeerId::from(id), format!("email [{email}] expected to return id [{id}]"));
	}
	let cases = [
		"someone@otherdomain.net",
		"@example.com",             // empty local part
		"some@one@example.com",     // more than one '@'
		"someone@example.com.evil",
		"someone@example.org",
	];
	for email in cases {
		ensure!(server.get_id(email).is_err(), format!("this email should be rejected: {email}"));
	}
	Ok(())
}

#[test]
fn wrong_server_config () -> Result<()> {
	let configs = [
		"domains = []",
		"",
		"[recipents]\na = 1",
		r#"
		api_key = "test-api-key"
		api_gateway = "https://api.telegram.org"
		default = 0
		fields = ["date", "from", "subject"]
		domains = ["example.com"]
		# no recipients
		"#,
		r#"
		ap_key = "test-api-key" # bad one
		api_gateway = "https://api.telegram.org"
		default = 0
		fields = ["date", "from", "subject"]
		domains = ["example.com"]

		[recipients]
		"someone@example.com" = 1
		"root" = -1"#,
		r#"
		api_key = "test-api-key"
		api_gateway = "https://api.telegram.org"
		default = 0
		fields = ["date", "from", "subject"]
		domains = [] # empty

		[recipients]
		"someone@example.com" = 1
		"root" = -1"#,
	];
	for config in configs {
		let settings = config::Config::builder()
			.add_source(config::File::from_str(config, Toml))
			.build()
			.stack()?;
		ensure!(MailServer::new(settings).is_err(), format!("this config shouldn't be valid:\n{config}"));
	}
	
	Ok(())
}
