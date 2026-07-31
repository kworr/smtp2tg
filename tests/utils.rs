use smtp2tg::utils::{
	validate,
	RE_CLOSING,
	RE_DOMAIN,
};
use stacked_errors::Result;
use std::borrow::Cow;

#[test]
fn test_validate_valid_html() -> Result<()> {
	let html = "<p>Some <b>valid</b> HTML</p>";
	let escaped = validate(html)?;
	assert_eq!(escaped, "&lt;p&gt;Some &lt;b&gt;valid&lt;/b&gt; HTML&lt;/p&gt;");
	Ok(())
}

#[test]
fn test_validate_closing_tag() -> Result<()> {
	let html = "<p>Some <b>valid</b> HTML</p></code><a href='http://somewere.com'>Link injection!</a>";
	assert!(validate(html).is_err());
    assert!(validate("</pre>").is_err());
    assert!(validate("</code>").is_err());
    assert!(validate("</pre>\n").is_err());
    assert!(validate("</code>\t").is_err());
	Ok(())
}

#[test]
fn test_validate_empty_string() -> Result<()> {
	assert_eq!(validate("").unwrap(), "");
	Ok(())
}

#[test]
fn test_validate_whitespace() -> Result<()> {
	assert_eq!(validate("   \t\n").unwrap(), "   \t\n"); // no escaping for whitespace
	Ok(())
}

#[test]
fn test_regex_closing_tag_matches() {
	assert!(RE_CLOSING.is_match("</pre>"));
	assert!(RE_CLOSING.is_match("</code>\t>"));
	assert!(!RE_CLOSING.is_match("</div>")); // Not a pre/code tag
}

#[test]
fn test_regex_domain_matches() {
	assert!(RE_DOMAIN.is_match("example.com"));
	assert!(RE_DOMAIN.is_match("sub.example.co.uk"));
	assert!(!RE_DOMAIN.is_match("invalid@domain.com"));
}

// `validate` returns `Cow<'a, str>` borrowed from its input lifetime `'a`.
// These two tests exercise both branches of that `Cow` to make sure the
// explicit lifetime introduced on `validate` still lets callers observe a
// zero-copy borrow when no escaping is required.
#[test]
fn test_validate_borrows_when_no_escaping_needed() -> Result<()> {
	let text = "plain text without special html characters";
	match validate(text)? {
		Cow::Borrowed(s) => assert_eq!(s, text),
		Cow::Owned(_) => panic!("expected a borrowed Cow when no escaping is needed"),
	}
	Ok(())
}

#[test]
fn test_validate_owns_when_escaping_needed() -> Result<()> {
	let text = "5 > 3 & 2 < 4";
	match validate(text)? {
		Cow::Owned(s) => assert_eq!(s, "5 &gt; 3 &amp; 2 &lt; 4"),
		Cow::Borrowed(_) => panic!("expected an owned Cow when escaping is performed"),
	}
	Ok(())
}

#[test]
fn test_regex_domain_rejects_invalid_formats() {
	assert!(!RE_DOMAIN.is_match(""));
	assert!(!RE_DOMAIN.is_match("-example.com"));
	assert!(!RE_DOMAIN.is_match("example-.com"));
	assert!(!RE_DOMAIN.is_match("example..com"));
	assert!(!RE_DOMAIN.is_match(".example.com"));
	assert!(!RE_DOMAIN.is_match("example.com."));
	assert!(!RE_DOMAIN.is_match("EXAMPLE.COM"));
}

#[test]
fn test_regex_domain_accepts_edge_cases() {
	assert!(RE_DOMAIN.is_match("a"));
	assert!(RE_DOMAIN.is_match("a.b"));
	assert!(RE_DOMAIN.is_match("my-host.example.com"));
	assert!(RE_DOMAIN.is_match("123.456"));
}

#[test]
fn test_regex_closing_tag_variants() {
	assert!(RE_CLOSING.is_match("</pre>"));
	assert!(RE_CLOSING.is_match("</  pre  >"));
	assert!(RE_CLOSING.is_match("</\tcode\t>"));
	assert!(!RE_CLOSING.is_match("<pre>"));
	assert!(!RE_CLOSING.is_match("</PRE>"));
	assert!(!RE_CLOSING.is_match("</b>"));
}
