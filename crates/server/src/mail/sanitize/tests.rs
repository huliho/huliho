// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use super::*;

const INSTANCE: &str = "https://mail.example.test";

fn plain() -> Sanitizer {
    Sanitizer::new(None)
}

fn instance() -> Sanitizer {
    Sanitizer::new(Some(&Url::parse(INSTANCE).unwrap()))
}

fn link(value: &str) -> String {
    plain().clean(&format!("<a href=\"{value}\">x</a>"))
}

fn image(value: &str) -> String {
    plain().clean(&format!("<img src=\"{value}\">"))
}

fn cell(element: &str, value: &str) -> String {
    let html = match element {
        "table" => format!("<table background=\"{value}\"><tr><td>x</td></tr></table>"),
        "tr" => format!("<table><tr background=\"{value}\"><td>x</td></tr></table>"),
        _ => format!("<table><tr><{element} background=\"{value}\">x</{element}></tr></table>"),
    };
    plain().clean(&html)
}

#[test]
fn a_script_an_event_handler_and_a_form_leave_and_the_text_stays() {
    let html = "<p onclick=\"top.__x=1\">Hi</p><script>top.__x=1</script>\
        <form action=\"https://evil.example/\"><input name=\"q\"><button>Go</button></form>";
    assert_eq!(plain().clean(html), "<p>Hi</p>Go");
}

#[test]
fn every_forbidden_element_leaves() {
    for html in [
        "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example/\">",
        "<link rel=\"stylesheet\" href=\"https://evil.example/x.css\">",
        "<base href=\"https://evil.example/\">",
        "<iframe src=\"https://evil.example/\"></iframe>",
        "<object data=\"https://evil.example/x\"></object>",
        "<embed src=\"https://evil.example/x\">",
        "<svg><script>top.__x=1</script></svg>",
        "<math><mi>x</mi></math>",
        "<template><img src=\"x\" onerror=\"top.__x=1\"></template>",
        "<video src=\"https://evil.example/v\"></video>",
        "<audio src=\"https://evil.example/a\"></audio>",
        "<picture><source srcset=\"https://evil.example/x\"></picture>",
        "<select><option>x</option></select>",
        "<textarea>x</textarea>",
        "<title>T</title>",
    ] {
        let cleaned = plain().clean(html);
        assert!(!cleaned.contains('<'), "{html} -> {cleaned}");
    }
}

#[test]
fn links_open_in_a_new_tab_without_an_opener_whatever_the_sender_wrote() {
    let html = "<a href=\"https://x.example/\" target=\"_self\" rel=\"opener\" \
        ping=\"https://t.example/\">x</a><a name=\"top\">y</a>";
    assert_eq!(
        plain().clean(html),
        "<a href=\"https://x.example/\" target=\"_blank\" rel=\"noopener noreferrer\">x</a>\
        <a target=\"_blank\" rel=\"noopener noreferrer\">y</a>"
    );
}

#[test]
fn a_link_keeps_http_https_and_mailto_and_nothing_else() {
    for value in [
        "https://x.example/a?b=c#d",
        "http://x.example/",
        "mailto:a@x.example?subject=hi",
        "HTTPS://X.EXAMPLE/",
    ] {
        assert!(link(value).contains("href="), "{value}");
    }
    for value in [
        "cid:part@x",
        "data:image/png;base64,AAAA",
        "data:text/html,<script>top.__x=1</script>",
        "javascript:top.__x=1",
        "JavaScript:top.__x=1",
        "java\tscript:top.__x=1",
        " javascript:top.__x=1",
        "&#106;avascript:top.__x=1",
        "ftp://x.example/",
        "tel:+3120",
        "/settings",
        "settings",
        "#top",
        "//x.example/",
        "/api/session",
        "",
    ] {
        assert!(!link(value).contains("href="), "{value}");
    }
}

#[test]
fn an_image_keeps_http_https_cid_and_a_data_image_and_nothing_else() {
    for value in [
        "https://x.example/a.png",
        "http://x.example/a.png",
        "cid:part1@x.example",
        "data:image/png;base64,AAAA",
        "data:IMAGE/GIF;base64,AAAA",
    ] {
        assert!(image(value).contains("src="), "{value}");
    }
    for value in [
        "mailto:a@x.example",
        "data:text/html,<script>top.__x=1</script>",
        "data:,x",
        "javascript:top.__x=1",
        "/api/remote-image?url=https://x.example/a.png",
        "a.png",
        "//x.example/a.png",
        "",
    ] {
        assert!(!image(value).contains("src="), "{value}");
    }
}

#[test]
fn background_follows_the_image_policy_on_table_rows_and_cells_alone() {
    for element in ["table", "tr", "td"] {
        assert!(
            cell(element, "https://x.example/bg.png").contains("background="),
            "{element}"
        );
        assert!(
            cell(element, "cid:bg@x.example").contains("background="),
            "{element}"
        );
        for value in [
            "javascript:top.__x=1",
            "bg.png",
            "/api/x",
            "mailto:a@x.example",
        ] {
            assert!(
                !cell(element, value).contains("background="),
                "{element} {value}"
            );
        }
    }
    assert!(!cell("th", "https://x.example/bg.png").contains("background="));
    let div = plain().clean("<div background=\"https://x.example/bg.png\">x</div>");
    assert_eq!(div, "<div>x</div>");
}

#[test]
fn the_instances_own_host_and_any_api_path_are_dropped_on_every_url_attribute() {
    let instance = instance();
    for html in [
        "<img src=\"https://mail.example.test/logo.png\">",
        "<img src=\"https://MAIL.example.test/logo.png\">",
        "<a href=\"https://mail.example.test/settings\">x</a>",
        "<table><tr><td background=\"https://mail.example.test/bg.png\">x</td></tr></table>",
        "<img src=\"https://cdn.example/api/remote-image?url=x\">",
        "<a href=\"https://cdn.example/api/x\">x</a>",
    ] {
        let cleaned = instance.clean(html);
        assert!(
            !cleaned.contains("mail.example.test"),
            "{html} -> {cleaned}"
        );
        assert!(!cleaned.contains("/api/"), "{html} -> {cleaned}");
    }
    assert!(
        instance
            .clean("<img src=\"https://cdn.example/a.png\">")
            .contains("src=")
    );
    assert!(
        plain()
            .clean("<img src=\"https://mail.example.test/logo.png\">")
            .contains("src=")
    );
}

#[test]
fn style_blocks_and_attributes_pass_as_text_for_the_frames_policy_to_gate() {
    let html = "<style>@import url(https://evil.example/x.css); \
        .a{background:url(https://evil.example/p.gif)}</style>\
        <p style=\"color:red;background:url(https://evil.example/q.gif)\">x</p>";
    assert_eq!(plain().clean(html), html);
}

#[test]
fn comments_srcset_ping_and_handlers_in_any_casing_leave() {
    let html = "<!-- c --><img src=\"https://x.example/a.png\" srcset=\"https://evil.example/b.png 2x\" \
        ONERROR=\"top.__x=1\" OnLoad=\"top.__x=1\" onerror=\"top.__x=1\">\
        <!--[if IE]><script>top.__x=1</script><![endif]-->";
    assert_eq!(plain().clean(html), "<img src=\"https://x.example/a.png\">");
}

#[test]
fn head_content_and_the_document_elements_never_survive() {
    let html = "<!DOCTYPE html><html onload=\"top.__x=1\"><head><title>T</title>\
        <meta charset=\"utf-8\"><base href=\"https://evil.example/\"><style>p{color:red}</style>\
        </head><body onload=\"top.__x=1\" bgcolor=\"#eeeeee\"><p>x</p></body></html>";
    assert_eq!(plain().clean(html), "<style>p{color:red}</style><p>x</p>");
}

#[test]
fn a_style_inside_a_foreign_element_leaves_with_its_content() {
    let html = "<svg><style><img src=x onerror=top.__x=1></style></svg>\
        <math><mtext><table><mglyph><style><img src=x onerror=top.__x=1></style></mglyph>\
        </table></mtext></math>";
    let cleaned = plain().clean(html);
    assert!(!cleaned.contains("onerror"), "{cleaned}");
    assert!(!cleaned.contains("<style"), "{cleaned}");
}

#[test]
fn text_that_is_not_html_comes_back_escaped_so_the_pass_keeps_plain_parts_out() {
    assert_eq!(plain().clean("a < b & c"), "a &lt; b &amp; c");
}

#[test]
fn the_builder_carries_the_allowlist_and_nothing_of_the_library_defaults() {
    let sanitizer = plain();
    let builder = &sanitizer.builder;
    assert_eq!(builder.clone_tags(), TAGS.iter().copied().collect());
    assert_eq!(
        builder.clone_clean_content_tags(),
        CLEAN_CONTENT_TAGS.iter().copied().collect()
    );
    assert_eq!(
        builder.clone_generic_attributes(),
        GENERIC_ATTRIBUTES.iter().copied().collect()
    );
    assert_eq!(builder.clone_tag_attributes(), tag_attributes());
    assert!(builder.clone_tag_attribute_values().is_empty());
    assert_eq!(builder.clone_generic_attribute_prefixes(), None);
    assert_eq!(
        builder.clone_url_schemes(),
        URL_SCHEMES.iter().copied().collect()
    );
    assert!(builder.is_url_relative_deny());
    assert_eq!(builder.get_link_rel(), Some(LINK_REL));
    assert!(builder.will_strip_comments());
    assert!(builder.clone_allowed_classes().is_empty());
    assert_eq!(
        builder.get_set_tag_attribute_value("a", "target"),
        Some(LINK_TARGET)
    );
    for forbidden in [
        "script", "iframe", "object", "embed", "form", "input", "button", "textarea", "select",
        "meta", "link", "base", "svg", "math", "template", "video", "audio", "source", "picture",
        "html", "head", "body", "title",
    ] {
        assert!(!TAGS.contains(&forbidden), "{forbidden}");
    }
    for forbidden in [
        "srcset",
        "ping",
        "target",
        "rel",
        "action",
        "formaction",
        "xlink:href",
    ] {
        assert!(!GENERIC_ATTRIBUTES.contains(&forbidden), "{forbidden}");
    }
    assert!(
        GENERIC_ATTRIBUTES
            .iter()
            .all(|name| !name.starts_with("on"))
    );
}
