use smtp2tg::utils::{
	validate,
	RE_CLOSING,
	RE_DOMAIN,
};
use stacked_errors::Result;

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
