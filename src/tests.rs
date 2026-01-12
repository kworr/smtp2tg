use crate::utils::validate;

use stacked_errors::{
	Result,
	StackableErr,
};

#[test]
fn check_valid () -> Result<()> {
	let html = "<p>Some <b>valid</b> HTML</p>";
	let res = validate(html).stack()?;
	assert_eq!(res, html);
	Ok(())
}

#[test]
#[should_panic = "Found special tag while closing generic tag"]
fn check_invalid () -> () {
	let html = "<p>Some <b>valid</b> HTML</p></code><a href='http://somewere.com'>Link injection!</a>";
	let _ = validate(html).unwrap();
	()
}
