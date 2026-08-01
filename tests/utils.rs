use smtp2tg::utils::{
	validate,
	RE_CLOSING,
	RE_DOMAIN,
};

use std::{
	borrow::Cow,
	mem::discriminant,
};

use stacked_errors::{
	Result,
	ensure_eq,
};

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
		ensure_eq!(&result, expected, format!("unexpected output for input {input:?}"));
		ensure_eq!(discriminant(&result), discriminant(expected), format!("wrong Cow variant for input {input:?}"));
	}
	Ok(())
}

#[test]
fn test_validate_closing_tag_behavior () -> Result<()> {
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
		ensure_eq!(RE_CLOSING.is_match(input), expected, format!("unexpected match result for {input:?}"));
	}
	Ok(())
}

#[test]
fn test_regex_domain_behavior() -> Result<()> {
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
		ensure_eq!(RE_DOMAIN.is_match(input), expected, format!("unexpected match result for {input:?}"));
	}
	Ok(())
}
