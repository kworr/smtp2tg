//! SMTP-to-Telegram gateway main library.
//!
//! This module provides the core functionality for receiving emails via SMTP
//! and forwarding them to Telegram chats.
//!
//! As we are not actually exporting this lib there would be no local Error's
//! for now, everything will be just .stack()?'ed and propagated like in real
//! bin The lib here is just to separate all tests from main code into tests/

pub mod mail;
mod telegram;
pub mod utils;

use crate::mail::MailServer;

use std::{
	io::Cursor,
	os::unix::fs::PermissionsExt,
	path::Path,
};

use clap::Parser;
use smol::{
	fs::metadata,
};
use stacked_errors::{
	Result,
	StackableErr,
	bail,
};

/// SMTP-to-Telegram gateway
#[derive(Parser, Debug)]
#[command(name = "smtp2tg")]
#[command(about = format!("SMTP-to-Telegram gateway v{}, (C) 2024 - 2026", env!("CARGO_PKG_VERSION")), long_about = None)]
struct Args {
	/// Set configuration file location
	#[arg(short, long, default_value = "smtp2tg.toml")]
	config: String,
}

/// Runs the gateway using the configuration selected on the command line.
///
/// The configuration file must use owner-only permissions. Once configured,
/// the SMTP server runs until it stops or encounters an error.
///
/// # Errors
/// Returns an error if the configuration file is missing, inaccessible,
/// insecure, malformed, or contains invalid required settings.
///
/// # Panics
/// Panics if configuring or serving the SMTP server fails.
pub async fn async_main () -> Result<()> {
	let args = Args::parse();
	let config_file = Path::new(&args.config);
	if !config_file.exists() {
		bail!("Configuration file not found: {config_file:?}\n\
			Hint: Ensure the file exists and the path is correct.");
	};
	{
		let meta = metadata(config_file).await.stack()?;
		if (!0o100600 & meta.permissions().mode()) > 0 {
			bail!("Configuration file permissions are insecure {config_file:?}\n\
				Current permissions: {:o}\n\
				Required: 0600 (owner read/write only).\n\
				Fix with: chmod 600 {config_file:?}",
				meta.permissions().mode());
	}	}
	let settings: config::Config = config::Config::builder()
		.set_default("api_gateway", "https://api.telegram.org").stack()?
		.set_default("fields", vec!["date", "from", "subject"]).stack()?
		.set_default("hostname", "smtp.2.tg").stack()?
		.set_default("listen_on", "0.0.0.0:1025").stack()?
		.set_default("domains", vec!["localhost",
			hostname::get().context("Failed to get current hostname")?
			.to_str().context("Can't convert hostname to string, bad UTF-8?")?]).stack()?
		.add_source(config::File::from(config_file))
		.build()
		.with_context(|| format!(
			"Failed to parse configuration file: {config_file:?}\n\
			Check syntax against smtp2tg.toml.example.\n\
			Common issues: missing quotes, trailing commas, or invalid types."
		))?;

	let listen_on = settings.get_string("listen_on").stack()?;
	let server_name = settings.get_string("hostname").stack()?;
	let core = MailServer::new(settings)?;
	let mut server = mailin_embedded::Server::new(core);

	// TODO: remove unwraps when mailin-embedded bumps with better error handling
	server.with_name(server_name)
		.with_ssl(mailin_embedded::SslConfig::None).unwrap()
		.with_addr(listen_on).unwrap();
	server.serve().unwrap();

	Ok(())
}
