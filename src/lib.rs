//! SMTP-to-Telegram gateway main library.
//!
//! This lib provides the core functionality for receiving emails via SMTP
//! and forwarding them to Telegram chats.
//!
//! As we are not actually exporting this lib there would be no local Error's
//! for now, everything will be just .stack()?'ed and propagated like in real
//! bin. The lib here is just to separate all tests from main code into tests/

pub mod mail;
mod telegram;
pub mod utils;

use std::io::Cursor;
