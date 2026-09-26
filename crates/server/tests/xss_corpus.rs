// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The XSS corpus through the server layer of the sanitizer: the
//! `DOMPurify` fixtures and the mail cases each come back within the
//! allowlist, as the same tree after a second pass and with what they
//! keep still there; so does any document a grammar of the bad shapes
//! produces.

mod html_check;

use std::sync::LazyLock;

use huliho_server::mail::sanitize::Sanitizer;
use proptest::prelude::*;
use serde::Deserialize;
use url::Url;

const DOMPURIFY: &str = include_str!("xss/dompurify-expect.json");
const MAIL: &str = include_str!("xss/mail.json");

/// The instance the corpus runs on, so the own-host cases bite.
const INSTANCE: &str = "https://mail.example.test";
const OWN_HOST: &str = "mail.example.test";

/// The `DOMPurify` fixtures hold at least this many cases.
const DOMPURIFY_FLOOR: usize = 200;

/// The elements the grammar opens and closes: kept, dropped, raw text,
/// foreign and the integration points between them.
const ELEMENTS: &[&str] = &[
    "a",
    "p",
    "div",
    "img",
    "table",
    "tr",
    "td",
    "style",
    "script",
    "svg",
    "math",
    "iframe",
    "form",
    "input",
    "template",
    "title",
    "textarea",
    "noscript",
    "mtext",
    "mglyph",
    "annotation-xml",
    "foreignObject",
    "body",
    "head",
    "meta",
    "base",
    "select",
    "xmp",
];

const ATTRIBUTES: &[&str] = &[
    "href",
    "src",
    "background",
    "onerror",
    "onclick",
    "ONLOAD",
    "style",
    "srcset",
    "id",
    "class",
    "target",
    "rel",
    "action",
    "xlink:href",
    "formaction",
    "ping",
    "encoding",
    "dir",
];

const VALUES: &[&str] = &[
    "https://cdn.example/a.png",
    "http://x.example/",
    "https://mail.example.test/api/x",
    "/api/remote-image?url=x",
    "cid:part@x",
    "data:image/png;base64,AAA",
    "data:text/html,<script>top.__x=1</script>",
    "javascript:top.__x=1",
    "mailto:a@b.example",
    "//evil.example/x",
    "#top",
    "top.__x=1",
    "text/html",
    "</style><img src=x onerror=top.__x=1>",
    "\"><img src=x onerror=top.__x=1>",
];

static SANITIZER: LazyLock<Sanitizer> =
    LazyLock::new(|| Sanitizer::new(Some(&Url::parse(INSTANCE).unwrap())));

#[derive(Deserialize)]
struct Case {
    /// The `DOMPurify` fixtures leave some cases untitled.
    #[serde(default)]
    title: String,
    payload: String,
    /// Substrings the server layer keeps, so a case proves it is not
    /// over-sanitized.
    #[serde(default)]
    keeps: Vec<String>,
}

impl Case {
    fn label(&self) -> &str {
        if self.title.is_empty() {
            &self.payload
        } else {
            &self.title
        }
    }
}

fn cases(file: &str) -> Vec<Case> {
    serde_json::from_str(file).unwrap()
}

/// The cleaned payload once it passed every check. A second pass may
/// reorder the attributes of an element the first pass dropped one
/// from, so the passes compare as trees.
fn assert_case(case: &Case) -> String {
    let cleaned = SANITIZER.clean(&case.payload);
    if let Err(problem) = html_check::check(&cleaned, Some(OWN_HOST)) {
        panic!("{}: {problem}\n{cleaned}", case.label());
    }
    let again = SANITIZER.clean(&cleaned);
    assert_eq!(
        html_check::shape(&again),
        html_check::shape(&cleaned),
        "{}",
        case.label()
    );
    for kept in &case.keeps {
        assert!(
            cleaned.contains(kept),
            "{}: {kept} missing from {cleaned}",
            case.label()
        );
    }
    cleaned
}

#[test]
fn every_dompurify_fixture_comes_back_within_the_allowlist() {
    let cases = cases(DOMPURIFY);
    assert!(cases.len() >= DOMPURIFY_FLOOR, "{}", cases.len());
    for case in &cases {
        assert_case(case);
    }
}

#[test]
fn every_mail_case_comes_back_within_the_allowlist_and_keeps_what_it_should() {
    let cases = cases(MAIL);
    assert!(cases.iter().all(|case| !case.title.is_empty()));
    for case in &cases {
        assert_case(case);
    }
}

#[test]
fn the_canaries_of_the_mail_cases_never_survive_as_code() {
    for case in cases(MAIL) {
        let cleaned = assert_case(&case);
        assert!(!cleaned.contains("<script"), "{}: {cleaned}", case.title);
        assert!(
            !cleaned.contains("javascript:"),
            "{}: {cleaned}",
            case.title
        );
    }
}

fn piece() -> impl Strategy<Value = String> {
    let element = prop::sample::select(ELEMENTS);
    let attribute = (
        prop::sample::select(ATTRIBUTES),
        prop::sample::select(VALUES),
    )
        .prop_map(|(name, value)| format!(" {name}=\"{value}\""));
    prop_oneof![
        (element.clone(), prop::collection::vec(attribute, 0..3))
            .prop_map(|(name, attributes)| format!("<{name}{}>", attributes.concat())),
        element.prop_map(|name| format!("</{name}>")),
        Just("<!--".to_owned()),
        Just("-->".to_owned()),
        Just("<![CDATA[".to_owned()),
        Just("]]>".to_owned()),
        "[a-z <>&\"'=/]{0,12}",
    ]
}

fn documents() -> impl Strategy<Value = String> {
    prop::collection::vec(piece(), 1..16).prop_map(|pieces| pieces.concat())
}

proptest! {
    #[test]
    fn any_document_comes_back_within_the_allowlist(document in documents()) {
        let cleaned = SANITIZER.clean(&document);
        if let Err(problem) = html_check::check(&cleaned, Some(OWN_HOST)) {
            prop_assert!(false, "{problem}\n{document}\n{cleaned}");
        }
        let again = SANITIZER.clean(&cleaned);
        prop_assert_eq!(html_check::shape(&again), html_check::shape(&cleaned));
    }
}
