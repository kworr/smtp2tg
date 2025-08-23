//! Simple SMTP-to-Telegram gateway. Can parse email and send them as telegram
//! messages to specified chats, generally you specify which email address is
//! available in configuration, everything else is sent to default address.

mod mail;
mod telegram;
mod utils;

#[cfg(test)]
mod tests;

use crate::mail::MailServer;

use async_std::fs::metadata;
use just_getopt::{
	OptFlags,
	OptSpecs,
	OptValue,
};
use stacked_errors::{
	Result,
	StackableErr,
	bail,
};

use std::{
	io::Cursor,
	os::unix::fs::PermissionsExt,
	path::Path,
};

#[async_std::main]
async fn main () -> Result<()> {
	let specs = OptSpecs::new()
		.option("help", "h", OptValue::None)
		.option("help", "help", OptValue::None)
		.option("config", "c", OptValue::Required)
		.option("config", "config", OptValue::Required)
		.flag(OptFlags::OptionsEverywhere);
	let mut args = std::env::args();
	args.next();
	let parsed = specs.getopt(args);
	for u in &parsed.unknown {
		println!("Unknown option: {u}");
	}
	if !(parsed.unknown.is_empty()) || parsed.options_first("help").is_some() {
		println!("SMTP2TG v{}, (C) 2024 - 2025\n\n\
			\t-h|--help\tDisplay this help\n\
			\t-c|--config …\tSet configuration file location.",
			env!("CARGO_PKG_VERSION"));
		return Ok(());
	};
	let config_file = Path::new(if let Some(path) = parsed.options_value_last("config") {
		&path[..]
	} else {
		"smtp2tg.toml"
	});
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
		.set_default("fields", vec!["date", "from", "subject"]).stack()?
		.set_default("hostname", "smtp.2.tg").stack()?
		.set_default("listen_on", "0.0.0.0:1025").stack()?
		.set_default("unknown", "relay").stack()?
		.set_default("domains", vec!["localhost", hostname::get().stack()?.to_str().expect("Failed to get current hostname")]).stack()?
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
