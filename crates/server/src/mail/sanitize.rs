// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The server layer of the mail sanitizer: one allowlist of elements,
//! attributes and URL schemes over an HTML5 parser, so a sender's HTML
//! leaves the proxy without a script, a form, an event handler or a URL
//! the frame would load with the reader's cookies. The window runs the
//! same allowlist once more before the frame renders the value.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ammonia::{Builder, UrlRelative};
use url::Url;

#[cfg(test)]
mod tests;

/// The elements a mail keeps: structure, text, lists, tables, inline
/// marks, images and its style blocks. Head content, scripts, form
/// controls, embedded documents and foreign namespaces stay out.
pub const TAGS: &[&str] = &[
    "a",
    "abbr",
    "article",
    "b",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "main",
    "mark",
    "nav",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "section",
    "small",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "time",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
];

/// The elements dropped with their content: a script's text is code
/// and a title's text is head content.
const CLEAN_CONTENT_TAGS: &[&str] = &["script", "title"];

/// The attributes any kept element may carry: styling, the identity a
/// mail's own selectors use, text direction and the presentational
/// attributes of tables and fonts. Never an event handler, `srcset`,
/// `ping`, `rel` or the sender's `target`.
pub const GENERIC_ATTRIBUTES: &[&str] = &[
    "align",
    "alt",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "class",
    "color",
    "colspan",
    "dir",
    "face",
    "height",
    "id",
    "lang",
    "rowspan",
    "size",
    "style",
    "title",
    "valign",
    "width",
];

/// The URL attributes, each on the elements that carry it.
pub const URL_ATTRIBUTES: &[(&str, &str)] = &[
    ("a", "href"),
    ("img", "src"),
    ("table", "background"),
    ("tr", "background"),
    ("td", "background"),
];

/// The schemes a URL attribute may carry at all; the policy narrows
/// them per attribute.
const URL_SCHEMES: &[&str] = &["cid", "data", "http", "https", "mailto"];

/// Every link opens in a new tab that knows nothing of this one.
pub const LINK_TARGET: &str = "_blank";
pub const LINK_REL: &str = "noopener noreferrer";

/// The path under which this server answers; a URL there would reach
/// the API with the reader's cookies.
const API_PATH: &str = "/api/";

/// The media type prefix a `data:` URL needs on an image.
const IMAGE_TYPE: &str = "image/";

/// The sanitizer, built once from the instance's own host.
pub struct Sanitizer {
    builder: Builder<'static>,
}

impl Sanitizer {
    /// For an instance reached on `public_url`; without one, a relative
    /// URL and an `/api/` path are the only ones read as the instance's
    /// own.
    #[must_use]
    pub fn new(public_url: Option<&Url>) -> Self {
        let own_host = public_url.and_then(Url::host_str).map(str::to_owned);
        let mut builder = Builder::empty();
        builder
            .tags(TAGS.iter().copied().collect())
            .clean_content_tags(CLEAN_CONTENT_TAGS.iter().copied().collect())
            .generic_attributes(GENERIC_ATTRIBUTES.iter().copied().collect())
            .tag_attributes(tag_attributes())
            .url_schemes(URL_SCHEMES.iter().copied().collect())
            .url_relative(UrlRelative::Deny)
            .link_rel(Some(LINK_REL))
            .strip_comments(true)
            .set_tag_attribute_value("a", "target", LINK_TARGET)
            .attribute_filter(move |element, attribute, value| {
                filter(element, attribute, value, own_host.as_deref())
            });
        Self { builder }
    }

    /// The HTML with everything off the allowlist removed.
    #[must_use]
    pub fn clean(&self, html: &str) -> String {
        self.builder.clean(html).to_string()
    }
}

fn tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (element, attribute) in URL_ATTRIBUTES {
        attributes.entry(element).or_default().insert(attribute);
    }
    attributes
}

/// What a URL attribute is for: a link the reader follows or an image
/// the frame loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Link,
    Image,
}

/// The purpose of a URL attribute; `None` for one that holds no URL.
fn purpose(element: &str, attribute: &str) -> Option<Purpose> {
    let purpose = if attribute == "href" {
        Purpose::Link
    } else {
        Purpose::Image
    };
    URL_ATTRIBUTES
        .contains(&(element, attribute))
        .then_some(purpose)
}

/// The URL policy, applied once every attribute off the allowlist is
/// gone: `http` and `https` off the instance on any URL attribute,
/// `mailto` on a link, `cid` and a `data:image/` URL on an image. A
/// relative URL, one on the instance's own host, any `/api/` path and
/// every other scheme are dropped, since the frame's own-origin image
/// source would load them with the reader's cookies.
fn filter<'u>(
    element: &str,
    attribute: &str,
    value: &'u str,
    own_host: Option<&str>,
) -> Option<Cow<'u, str>> {
    let Some(purpose) = purpose(element, attribute) else {
        return Some(Cow::Borrowed(value));
    };
    admitted(purpose, value, own_host).then_some(Cow::Borrowed(value))
}

fn admitted(purpose: Purpose, value: &str, own_host: Option<&str>) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    match url.scheme() {
        "http" | "https" => !on_instance(&url, own_host),
        "mailto" => purpose == Purpose::Link,
        "cid" => purpose == Purpose::Image,
        "data" => purpose == Purpose::Image && is_image(&url),
        _ => false,
    }
}

/// Whether the URL points at this server. A path under `/api/` counts
/// on any host, since an instance answers under more than one name;
/// so does the host the config names.
fn on_instance(url: &Url, own_host: Option<&str>) -> bool {
    url.path().starts_with(API_PATH) || own_host.is_some_and(|host| url.host_str() == Some(host))
}

/// A `data:` URL whose media type is an image, read case insensitively
/// (RFC 2397 section 3).
fn is_image(url: &Url) -> bool {
    url.path()
        .get(..IMAGE_TYPE.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(IMAGE_TYPE))
}
