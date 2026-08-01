# smtp2tg

SMTP-to-Telegram gateway. Receives emails via SMTP and forwards them to Telegram chats.

---

## Installation

### From source
```bash
git clone https://github.com/kworr/smtp2tg
cd smtp2tg
cargo build --release
```

## Configuration

1. Create `smtp2tg.toml` (see `smtp2tg.toml.example` for reference).
2. Get a Telegram Bot Token from [@BotFather](https://t.me/BotFather).
3. Get your chat ID (use [@getidsbot](https://t.me/getidsbot) or debug mode in Telegram client).

### Example configuration
```toml
api_key = "replace-with-your-telegram-bot-token"
api_gateway = "https://api.telegram.org"
listen_on = "127.0.0.1:1025"
unknown = "relay"
fields = ["date", "from", "subject"]
domains = ["example.com", "localhost"]

default = 0

[recipients]
"admin@example.com" = 12345678
"alerts@example.com" = -10012345678
```

To catch bounces (so they wouldn't stuck in upper mail server) make sure sender
envelope address is real as required by mail library (actually not sure whether
this applies to mailin). For example Postfix has to be tweaked like this:

$config_directory/main.cf:
	smtp_generic_maps = hash:$config_directory/generic

$config_directory/generic:
	""	postmaster@example.com
	<>	postmaster@example.com

Actually not sure which one works...

---

## Usage

### Run
```bash
./smtp2tg -c /path/to/smtp2tg.toml
```

### CLI arguments

| Argument      | Description               | Example               |
|---------------|---------------------------|-----------------------|
| `-h`, `--help` | Show help                 | `smtp2tg --help`      |
| `-c`, `--config` | Path to config file     | `smtp2tg -c config.toml` |

---

## Security

### Important
- **Recommended usage**: Run on `127.0.0.1` (localhost) only.
- If exposing to a network:
  - **Restrict port access** via firewall.
  - **Use TLS** (e.g., via `stunnel` or `nginx`).
  - **Implement authentication** (this software does not provide it).

### Config file permissions
Set file permissions to `0600` (owner read/write only):
```bash
chmod 600 smtp2tg.toml
```

---
## How it works
1. A client (e.g., Postfix) sends an email to `listen_on` (e.g., `127.0.0.1:1025`).
2. smtp2tg parses the email and converts it to a Telegram message.
3. The message is sent to the specified chat (or `default` if address is unknown).

### Example: Email → Telegram
**Incoming email:**
```text
From: user@example.com
To: admin@example.com
Subject: Test

Hello, world!
```

**Telegram message:**
```html
<blockquote expandable>
<u><i>Subject:</i></u> <code>Test</code>
<u><i>From:</i></u> <code>user@example.com</code>
<u><i>Date:</i></u> <code>Mon, 01 Jan 2024 12:00:00 +0000</code>
</blockquote>
<pre>Hello, world!</pre>
```

---
## Links
- **Original repository**: [http://fs.b1t.name/smtp2tg](http://fs.b1t.name/smtp2tg)
- **GitHub Mirror**: [https://github.com/kworr/smtp2tg](https://github.com/kworr/smtp2tg)
