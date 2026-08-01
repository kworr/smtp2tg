use smtp2tg::mail::MailServer;

use mailin_embedded::{
	Handler,
	response::{
		NO_MAILBOX,
		OK,
	},
};
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

/// Builds a `MailServer` with a configurable "unknown" address policy and set
/// of allowed domains, used to exercise `rcpt`/`get_id` behavior beyond the
/// fixed defaults covered by `build_server`. No network access is performed.
fn build_server_with (unknown: &str, domains: &[&str]) -> Result<MailServer> {
	let domains = domains.iter().map(|d| format!("\"{d}\"")).collect::<Vec<_>>().join(", ");
	let toml = format!(r#"
		api_key = "test-api-key"
		api_gateway = "https://api.telegram.org"
		default = 0
		unknown = "{unknown}"
		fields = ["date", "from", "subject"]
		domains = [{domains}]

		[recipients]
		"someone@example.com" = 1
		"root" = -1
	"#);
	let settings = config::Config::builder()
		.add_source(config::File::from_str(&toml, config::FileFormat::Toml))
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
	let cases = [
		"someone@otherdomain.net",
	];
	for email in cases {
		assert!(server.get_id(email).unwrap_err().to_string().contains("Doesn't look like address from one of our domains."), "email [{email}] expected to fail");
	}
	Ok(())
}

#[test]
fn get_id_rejects_malformed_addresses () -> Result<()> {
	let server = build_server()?;
	// Address regex is `^[a-z0-9][-a-z0-9]*(@(domains))?$`: it is
	// case-sensitive (lowercase only), only allows '-' besides alphanumerics,
	// and the optional domain suffix must be exactly one of the configured
	// domains with nothing trailing.
	let cases = [
		"SOMEONE@example.com",      // uppercase local part
		"someone@EXAMPLE.COM",      // uppercase domain
		"some.one@example.com",     // '.' not allowed in local part
		"@example.com",             // empty local part
		"some@one@example.com",     // more than one '@'
		"someone@example.com.evil", // trailing characters after the domain
		"",                         // empty address
	];
	for email in cases {
		let err = server.get_id(email).unwrap_err().to_string();
		assert!(err.contains("Doesn't look like address from one of our domains."),
			"email [{email:?}] expected to fail, got: {err}");
	}
	Ok(())
}

#[test]
fn get_id_accepts_hyphenated_bare_username () -> Result<()> {
	let server = build_server()?;
	// Bare usernames (no "@domain" part) are accepted as long as they start
	// with an alphanumeric character and only contain letters, digits or '-'
	// afterwards; unregistered ones fall back to the default recipient.
	assert_eq!(*server.get_id("some-one-2")?, ChatPeerId::from(0));
	Ok(())
}

#[test]
fn get_id_supports_multiple_configured_domains () -> Result<()> {
	let server = build_server_with("relay", &["example.com", "example.org"])?;
	assert_eq!(*server.get_id("someone@example.com")?, ChatPeerId::from(1));
	assert_eq!(*server.get_id("someone@example.org")?, ChatPeerId::from(0));
	assert!(server.get_id("someone@other.net").is_err(), "domain outside configuration should be rejected");
	Ok(())
}

#[test]
fn get_id_behavior_is_independent_of_unknown_policy () -> Result<()> {
	// `get_id` only checks the address regex; the "unknown" (relay/deny)
	// policy is applied separately by `rcpt`/`relay_mail`, so it must not
	// change what `get_id` resolves to.
	let relay_server = build_server_with("relay", &["example.com"])?;
	let deny_server = build_server_with("deny", &["example.com"])?;
	for server in [&relay_server, &deny_server] {
		assert_eq!(*server.get_id("someone@example.com")?, ChatPeerId::from(1));
		assert_eq!(*server.get_id("unknown@example.com")?, ChatPeerId::from(0));
		assert!(server.get_id("someone@other.net").is_err());
	}
	Ok(())
}

#[test]
fn rcpt_allows_any_address_when_relay_enabled () -> Result<()> {
	let mut server = build_server_with("relay", &["example.com"])?;
	// When relaying is enabled, `rcpt` short-circuits on `self.relay` and
	// accepts every recipient, even addresses outside configured domains or
	// addresses that don't look like an address at all.
	assert_eq!(server.rcpt("someone@example.com"), OK);
	assert_eq!(server.rcpt("someone@other-domain.net"), OK);
	assert_eq!(server.rcpt("not a valid address"), OK);
	Ok(())
}

#[test]
fn rcpt_denies_addresses_outside_domains_when_relay_disabled () -> Result<()> {
	let mut server = build_server_with("deny", &["example.com"])?;
	// With relaying disabled, `rcpt` falls back to `get_id`: any address that
	// matches a configured domain (registered or not) is accepted, anything
	// else is rejected with NO_MAILBOX.
	assert_eq!(server.rcpt("someone@example.com"), OK);
	assert_eq!(server.rcpt("unknown@example.com"), OK);
	assert_eq!(server.rcpt("someone@other.net"), NO_MAILBOX);
	assert_eq!(server.rcpt("not a valid address"), NO_MAILBOX);
	Ok(())
}
