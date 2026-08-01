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

/// Main asynchronous entry point for the application.
///
/// Parses command-line arguments, loads configuration, and starts the SMTP
/// server.
///
/// # Errors
/// Returns an error if configuration is invalid, files are inaccessible, or
/// server fails to start.
pub async fn async_main () -> Result<()> {
	let args = Args::parse();
	let config_file = Path::new(&args.config);
	if !config_file.exists() {
		bail!("can't read configuration from {config_file:?}");
	};
	{
		let meta = metadata(config_file).await.stack()?;
		if (!0o100600 & meta.permissions().mode()) > 0 {
			bail!("other users can read or write config file {config_file:?}\n\
				File permissions: {:o}", meta.permissions().mode());
		}
	}
	let settings: config::Config = config::Config::builder()
		.set_default("api_gateway", "https://api.telegram.org").stack()?
		.set_default("fields", vec!["date", "from", "subject"]).stack()?
		.set_default("hostname", "smtp.2.tg").stack()?
		.set_default("listen_on", "0.0.0.0:1025").stack()?
		.set_default("domains", vec!["localhost",
			hostname::get().expect("Failed to get current hostname")
			.to_str().expect("Can't convert hostname to string, bad UTF-8?")]).stack()?
		.add_source(config::File::from(config_file))
		.build()
		.with_context(|| format!("[{config_file:?}] there was an error reading config\n\
			\tplease consult \"smtp2tg.toml.example\" for details"))?;

	let listen_on = settings.get_string("listen_on").stack()?;
	let server_name = settings.get_string("hostname").stack()?;
	let core = MailServer::new(settings)?;
	let mut server = mailin_embedded::Server::new(core);

	server.with_name(server_name)
		.with_ssl(mailin_embedded::SslConfig::None).unwrap()
		.with_addr(listen_on).unwrap();
	server.serve().unwrap();

	Ok(())
}
