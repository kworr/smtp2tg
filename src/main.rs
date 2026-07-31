//! Simple SMTP-to-Telegram gateway. Can parse email and send them as telegram
//! messages to specified chats, generally you specify which email address is
//! available in configuration, everything else is sent to default address.

use async_compat::Compat;
use stacked_errors:: Result;

// main function stub that executes main code from lib
fn main () -> Result<()> {
	smol::block_on(Compat::new(async {
		smtp2tg::async_main().await
	}))
}
