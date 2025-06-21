use crate::telegram::encode;

#[test]
fn check_regex () {
	let res = encode("-_*[]()~`>#+|{}.!");
	assert_eq!(res, "\\-\\_\\*\\[\\]\\(\\)\\~\\`\\>\\#\\+\\|\\{\\}\\.\\!");
}
