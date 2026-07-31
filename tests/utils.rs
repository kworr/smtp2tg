use smtp2tg::utils::{
	validate,
	RE_CLOSING,
	RE_DOMAIN,
};
use stacked_errors::Result;
use std::borrow::Cow;

/// Expected `Cow` variant returned by `validate` for a given case.
/// `Any` is used where the original tests only checked the resulting
/// string and did not assert on borrowed/owned status.
#[derive(Clone, Copy)]
enum CowKind {
	Any,
	Borrowed,
	Owned,
}

#[test]
fn test_validate_escaping_behavior() -> Result<()> {
	let cases: &[(&str, &str, CowKind)] = &[
		// `validate` escapes HTML special characters.
		("<p>Some <b>valid</b> HTML</p>", "&lt;p&gt;Some &lt;b&gt;valid&lt;/b&gt; HTML&lt;/p&gt;", CowKind::Any),
		// Empty input is returned unchanged.
		("", "", CowKind::Any),
		// Whitespace-only input needs no escaping.
		("   \t\n", "   \t\n", CowKind::Any),
		// `validate` returns `Cow<'a, str>` borrowed from its input lifetime `'a`.
		// These two cases exercise both branches of that `Cow` to make sure the
		// explicit lifetime introduced on `validate` still lets callers observe a
		// zero-copy borrow when no escaping is required.
		("plain text without special html characters", "plain text without special html characters", CowKind::Borrowed),
		("5 > 3 & 2 < 4", "5 &gt; 3 &amp; 2 &lt; 4", CowKind::Owned),
	];

	for &(input, expected, kind) in cases {
		let result = validate(input)?;
		assert_eq!(result.as_ref(), expected, "unexpected output for input {input:?}");
		match kind {
			CowKind::Borrowed => assert!(matches!(result, Cow::Borrowed(_)), "expected a borrowed Cow for input {input:?}"),
			CowKind::Owned => assert!(matches!(result, Cow::Owned(_)), "expected an owned Cow for input {input:?}"),
			CowKind::Any => {}
		}
	}
	Ok(())
}

#[test]
fn test_validate_rejects_closing_tags() {
	let inputs = [
		"<p>Some <b>valid</b> HTML</p></code><a href='http://somewere.com'>Link injection!</a>",
		"</pre>",
		"</code>",
		"</pre>\n",
		"</code>\t",
	];

	for input in inputs {
		assert!(validate(input).is_err(), "expected an error for input {input:?}");
	}
}

#[test]
fn test_regex_closing_tag_behavior() {
	let cases = [
		("</pre>", true),
		("</code>\t>", true),
		("</div>", false), // Not a pre/code tag
		("</  pre  >", true),
		("</\tcode\t>", true),
		("<pre>", false),
		("</PRE>", false),
		("</b>", false),
	];

	for (input, expected) in cases {
		assert_eq!(RE_CLOSING.is_match(input), expected, "unexpected match result for {input:?}");
	}
}

#[test]
fn test_regex_domain_behavior() {
	let cases = [
		("example.com", true),
		("sub.example.co.uk", true),
		("invalid@domain.com", false),
		("", false),
		("-example.com", false),
		("example-.com", false),
		("example..com", false),
		(".example.com", false),
		("example.com.", false),
		("EXAMPLE.COM", false),
		("a", true),
		("a.b", true),
		("my-host.example.com", true),
		("123.456", true),
	];

	for (input, expected) in cases {
		assert_eq!(RE_DOMAIN.is_match(input), expected, "unexpected match result for {input:?}");
	}
}
