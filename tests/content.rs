//! Content lint (DESIGN.md §9): every content file must validate; a typo in
//! a content file fails CI — that is the intended guardrail.

mod common;

#[test]
fn world_content_is_valid() {
    let problems = heart_of_africa::content::validate::validate_content(common::content_dir())
        .expect("content loads without schema errors");
    assert!(
        problems.is_empty(),
        "content lint problems:\n  {}",
        problems.join("\n  ")
    );
}
