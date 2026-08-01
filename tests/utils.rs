use smtp2tg::utils::{
	validate,
	RE_CLOSING,
	RE_DOMAIN,
};

use std::{
	borrow::Cow,
	mem::discriminant,
	thread,
};

use stacked_errors::Result;

#[test]
fn test_validate_escaping_behavior () -> Result<()> {
	let cases: &[(&str, Cow<str>)] = &[
		// `validate` escapes HTML special characters.
		("<p>Some <b>valid</b> HTML</p>", Cow::Owned("&lt;p&gt;Some &lt;b&gt;valid&lt;/b&gt; HTML&lt;/p&gt;".into())),
		// Empty input is returned unchanged.
		("", Cow::Borrowed("")),
		// Whitespace-only input needs no escaping.
		("   \t\n", Cow::Borrowed("   \t\n")),
		// `validate` returns `Cow<'a, str>` borrowed from its input lifetime `'a`.
		// These two cases exercise both branches of that `Cow` to make sure the
		// explicit lifetime introduced on `validate` still lets callers observe a
		// zero-copy borrow when no escaping is required.
		("plain text without special html characters", Cow::Borrowed("plain text without special html characters")),
		("5 > 3 & 2 < 4", Cow::Owned("5 &gt; 3 &amp; 2 &lt; 4".into())),
	];
	for (input, expected) in cases {
		let result = validate(input)?;
		assert_eq!(&result, expected, "unexpected output for input {input:?}");
		assert_eq!(discriminant(&result), discriminant(expected), "wrong Cow variant for input {input:?}");
	}
	Ok(())
}

#[test]
fn test_validate_closing_tag_behavior () {
	let cases = [
		("</  pre  >", true),
		("</\tcode\t>", true),
		("</b>", false),
		("</Code>", true),
		("</code>", true),
		("</code>\t", true),
		("</code>\t>", true),
		("</div>", false), // Not a pre/code tag
		("</PRE>", true),
		("</pre>", true),
		("</pre>\n", true),
		("<p>Some <b>valid</b> HTML</p></code><a href='http://somewere.com'>Link injection!</a>", true),
		("<pre>", false),
	];
	for (input, expected) in cases {
		assert_eq!(RE_CLOSING.is_match(input), expected, "unexpected match result for {input:?}");
	}
}

#[test]
fn test_regex_domain_behavior() {
	let cases = [
		("", false),
		("-example.com", false),
		(".example.com", false),
		("123.456", true),
		("EXAMPLE.COM", false),
		("a", true),
		("a.b", true),
		("example-.com", false),
		("example..com", false),
		("example.com", true),
		("example.com.", false),
		("invalid@domain.com", false),
		("my-host.example.com", true),
		("sub.example.co.uk", true),
	];
	for (input, expected) in cases {
		assert_eq!(RE_DOMAIN.is_match(input), expected, "unexpected match result for {input:?}");
	}
}

#[test]
fn statics_survive_concurrent_first_access_after_lazylock_migration () {
	// `RE_DOMAIN`/`RE_CLOSING` moved from the `lazy_static!` macro to
	// `std::sync::LazyLock`. Make sure concurrent, possibly-first-time access
	// from multiple threads still initializes them safely and consistently
	// (a regression check for that migration, not for the regex content
	// itself, which is covered above).
	let handles: Vec<_> = (0..8).map(|i| {
		thread::spawn(move || {
			let domain_ok = RE_DOMAIN.is_match("example.com");
			let closing_ok = RE_CLOSING.is_match("</pre>");
			let validated = validate(&format!("thread-{i} <b>data</b>"))
				.expect("validate should not fail on plain text")
				.into_owned();
			(domain_ok, closing_ok, validated)
		})
	}).collect();

	for (i, handle) in handles.into_iter().enumerate() {
		let (domain_ok, closing_ok, validated) = handle.join().expect("worker thread panicked");
		assert!(domain_ok, "RE_DOMAIN should match \"example.com\" from thread {i}");
		assert!(closing_ok, "RE_CLOSING should match \"</pre>\" from thread {i}");
		assert_eq!(validated, format!("thread-{i} &lt;b&gt;data&lt;/b&gt;"));
	}
}
