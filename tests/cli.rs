//! The entry point is private to the binary, so test its public CLI and SMTP
//! behavior with isolated configuration directories and bounded subprocesses.

use std::{
	fs::{self, OpenOptions},
	io::{BufRead, BufReader, Write},
	net::{SocketAddr, TcpListener, TcpStream},
	os::unix::fs::{OpenOptionsExt, PermissionsExt},
	path::PathBuf,
	process::{Child, Command, Output, Stdio},
	sync::atomic::{AtomicUsize, Ordering},
	thread,
	time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(5);

struct TestDir(PathBuf);

impl TestDir {
	fn new () -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		loop {
			let path = std::env::temp_dir().join(format!("smtp2tg-cli-{}-{}",
				std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
			match fs::create_dir(&path) {
				Ok(()) => return Self(path),
				Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
				Err(error) => panic!("Cannot create test directory: {error}"),
			}
		}
	}

	fn config (&self, name: &str, text: &str, mode: u32) {
		let path = self.0.join(name);
		let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600)
			.open(&path).unwrap();
		file.write_all(text.as_bytes()).unwrap();
		fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
	}

	fn spawn (&self, args: &[&str]) -> Process {
		Process(Some(Command::new(env!("CARGO_BIN_EXE_smtp2tg"))
			.current_dir(&self.0).args(args)
			.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
			.spawn().unwrap()))
	}

	fn run (&self, args: &[&str]) -> Output {
		self.spawn(args).output()
	}
}

impl Drop for TestDir {
	fn drop (&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

struct Process(Option<Child>);

impl Process {
	fn output (mut self) -> Output {
		let deadline = Instant::now() + DEADLINE;
		while self.0.as_mut().unwrap().try_wait().unwrap().is_none() {
			assert!(Instant::now() < deadline, "CLI did not exit within {DEADLINE:?}");
			thread::sleep(Duration::from_millis(10));
		}
		self.0.take().unwrap().wait_with_output().unwrap()
	}

	fn connect (&mut self, address: SocketAddr) -> TcpStream {
		let deadline = Instant::now() + DEADLINE;
		loop {
			assert!(self.0.as_mut().unwrap().try_wait().unwrap().is_none(),
				"SMTP server exited before accepting connections");
			if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) {
				stream.set_read_timeout(Some(DEADLINE)).unwrap();
				stream.set_write_timeout(Some(DEADLINE)).unwrap();
				return stream;
			}
			assert!(Instant::now() < deadline, "SMTP server did not start within {DEADLINE:?}");
			thread::sleep(Duration::from_millis(10));
		}
	}
}

impl Drop for Process {
	fn drop (&mut self) {
		if let Some(child) = &mut self.0 {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}

fn assert_error (output: Output, expected: &str) {
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(!output.status.success(), "Expected failure, stderr: {stderr}");
	assert!(stderr.contains(expected), "Missing {expected:?} in stderr: {stderr}");
}

#[test]
fn help_succeeds_without_a_configuration_file () {
	let dir = TestDir::new();
	for flag in ["-h", "--help"] {
		let output = dir.run(&[flag]);
		assert!(output.status.success());
		let help = String::from_utf8(output.stdout).unwrap();
		for expected in ["SMTP-to-Telegram gateway", env!("CARGO_PKG_VERSION"),
			"-c, --config", "[default: smtp2tg.toml]"] {
			assert!(help.contains(expected), "Missing {expected:?} in help: {help}");
		}
	}
}

#[test]
fn missing_default_and_explicit_configuration_paths_are_reported () {
	let dir = TestDir::new();
	for (args, path) in [
		(vec![], "smtp2tg.toml"),
		(vec!["-c", "missing file.toml"], "missing file.toml"),
		(vec!["--config", "missing file.toml"], "missing file.toml"),
	] {
		let output = dir.run(&args);
		let stderr = String::from_utf8_lossy(&output.stderr);
		assert!(stderr.contains(path), "Missing path in diagnostic: {stderr}");
		assert_error(output, "Configuration file not found");
	}
}

#[test]
fn invalid_arguments_fail_before_configuration_loading () {
	let dir = TestDir::new();
	for args in [vec!["-c"], vec!["--config"], vec!["--unknown-option"]] {
		let output = dir.run(&args);
		assert_eq!(output.status.code(), Some(2), "arguments: {args:?}");
		assert!(!String::from_utf8_lossy(&output.stderr).contains("Configuration file not found"));
	}
}

#[test]
fn insecure_permissions_are_rejected_before_parsing () {
	// Each forbidden permission is independent: group read/write/execute,
	// other read/write/execute, and owner execute.
	for mode in [0o640, 0o620, 0o610, 0o604, 0o602, 0o601, 0o700] {
		let dir = TestDir::new();
		dir.config("smtp2tg.toml", "this is not valid TOML", mode);
		let output = dir.run(&[]);
		let stderr = String::from_utf8_lossy(&output.stderr);
		assert!(stderr.contains("Required: 0600"), "mode {mode:o}: {stderr}");
		assert!(stderr.contains("chmod 600"), "mode {mode:o}: {stderr}");
		assert_error(output, "Configuration file permissions are insecure");
	}
}

#[test]
fn secure_malformed_config_reports_parse_context () {
	let dir = TestDir::new();
	// A spaces-containing path must be passed intact, and the selected file
	// must take precedence over an insecure default configuration.
	dir.config("smtp2tg.toml", "ignored", 0o644);
	dir.config("selected config.toml", "api_key = [", 0o600);
	for flag in ["-c", "--config"] {
		let output = dir.run(&[flag, "selected config.toml"]);
		let stderr = String::from_utf8_lossy(&output.stderr);
		assert!(stderr.contains("selected config.toml"));
		assert!(stderr.contains("smtp2tg.toml.example"));
		assert_error(output, "Failed to parse configuration file");
	}
}

#[test]
fn required_settings_remain_required_after_applying_defaults () {
	for (config, expected) in [
		("default = 0\n[recipients]\n", "api_key"),
		("api_key = 'test-api-key'\n[recipients]\n", "default"),
		("api_key = 'test-api-key'\ndefault = 0\n", "recipients"),
	] {
		let dir = TestDir::new();
		dir.config("smtp2tg.toml", config, 0o600);
		let output = dir.run(&[]);
		assert_error(output, expected);
	}
}

fn smtp_reply (stream: &mut BufReader<TcpStream>) -> String {
	let mut response = String::new();
	loop {
		let mut line = String::new();
		assert!(stream.read_line(&mut line).unwrap() > 0, "SMTP connection closed: {response}");
		let more = line.as_bytes().get(3) == Some(&b'-');
		response.push_str(&line);
		if !more {
			return response;
		}
	}
}

#[test]
fn entry_point_serves_smtp_with_defaults_and_configuration_overrides () {
	for (overrides, hostname, allowed, denied) in [
		("", "smtp.2.tg", "localhost", "outside.invalid"),
		("hostname = 'custom.smtp'\ndomains = ['example.com']\nfields = []\n",
			"custom.smtp", "example.com", "localhost"),
	] {
		let dir = TestDir::new();
		let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
		let address = reservation.local_addr().unwrap();
		dir.config("smtp2tg.toml", &format!(
			"api_key = 'test-api-key'\ndefault = 0\nlisten_on = '{address}'\n{overrides}[recipients]\n"), 0o600);
		drop(reservation);
		let mut process = dir.spawn(&[]);
		let mut stream = BufReader::new(process.connect(address));
		let banner = smtp_reply(&mut stream);
		assert!(banner.starts_with("220 "), "{banner}");
		assert!(banner.contains(hostname), "{banner}");
		for (command, code) in [
			("EHLO test.local\r\n".into(), "250"),
			("MAIL FROM:<sender@example.org>\r\n".into(), "250"),
			(format!("RCPT TO:<user@{allowed}>\r\n"), "250"),
			(format!("RCPT TO:<user@{denied}>\r\n"), "550"),
			("QUIT\r\n".into(), "221"),
		] {
			stream.get_mut().write_all(command.as_bytes()).unwrap();
			let reply = smtp_reply(&mut stream);
			assert!(reply.starts_with(code), "command {command:?}: {reply}");
		}
	}
}
