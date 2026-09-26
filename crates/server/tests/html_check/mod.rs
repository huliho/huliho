// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a browser would build from a sanitized value, read through the
//! HTML5 parser once more: every element with its namespace and its
//! attributes, so a test can assert that nothing off the allowlist
//! survives the serialization.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use html5ever::interface::{
    Attribute, ElementFlags, ExpandedName, NodeOrText, QuirksMode, TreeSink,
};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{ParseOpts, QualName, local_name, ns, parse_fragment};
use huliho_server::mail::sanitize::{
    GENERIC_ATTRIBUTES, LINK_REL, LINK_TARGET, TAGS, URL_ATTRIBUTES,
};
use url::Url;

/// Elements the allowlist must never name, whatever it says.
const FORBIDDEN_ELEMENTS: &[&str] = &[
    "script", "iframe", "object", "embed", "form", "input", "button", "textarea", "select", "meta",
    "link", "base", "svg", "math", "template", "video", "audio", "source", "picture", "html",
    "head", "body", "title", "noscript", "frame", "frameset", "applet",
];

/// The schemes a link may carry, then the ones an image may carry.
const LINK_SCHEMES: &[&str] = &["http", "https", "mailto"];
const IMAGE_SCHEMES: &[&str] = &["http", "https", "cid", "data"];

/// The elements the fragment parser creates before it reads any input:
/// the context and the root.
const SYNTHETIC_ELEMENTS: usize = 2;

/// A node as the recorder keeps it: an element's name and attributes,
/// nothing for a text, a comment or the document.
pub struct Node {
    name: Option<QualName>,
    attributes: RefCell<Vec<Attribute>>,
}

type Handle = Rc<Node>;

/// A tree sink that keeps no tree: every element the parser creates in
/// its order plus the number of comments.
struct Recorder {
    document: Handle,
    elements: RefCell<Vec<Handle>>,
    comments: Cell<usize>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            document: plain(),
            elements: RefCell::new(Vec::new()),
            comments: Cell::new(0),
        }
    }
}

fn plain() -> Handle {
    Rc::new(Node {
        name: None,
        attributes: RefCell::new(Vec::new()),
    })
}

impl TreeSink for Recorder {
    type Handle = Handle;
    type Output = Self;
    type ElemName<'a>
        = ExpandedName<'a>
    where
        Self: 'a;

    fn finish(self) -> Self {
        self
    }

    fn parse_error(&self, _: Cow<'static, str>) {}

    fn get_document(&self) -> Handle {
        Rc::clone(&self.document)
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> ExpandedName<'a> {
        target
            .name
            .as_ref()
            .expect("the parser asks the name of an element")
            .expanded()
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, _: ElementFlags) -> Handle {
        let node = Rc::new(Node {
            name: Some(name),
            attributes: RefCell::new(attrs),
        });
        self.elements.borrow_mut().push(Rc::clone(&node));
        node
    }

    fn create_comment(&self, _: StrTendril) -> Handle {
        self.comments.set(self.comments.get() + 1);
        plain()
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> Handle {
        plain()
    }

    fn append(&self, _: &Handle, _: NodeOrText<Handle>) {}

    fn append_based_on_parent_node(&self, _: &Handle, _: &Handle, _: NodeOrText<Handle>) {}

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, _: &Handle) -> Handle {
        plain()
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        Rc::ptr_eq(x, y)
    }

    fn set_quirks_mode(&self, _: QuirksMode) {}

    fn append_before_sibling(&self, _: &Handle, _: NodeOrText<Handle>) {}

    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        let mut present = target.attributes.borrow_mut();
        for attribute in attrs {
            if !present.iter().any(|known| known.name == attribute.name) {
                present.push(attribute);
            }
        }
    }

    fn remove_from_parent(&self, _: &Handle) {}

    fn reparent_children(&self, _: &Handle, _: &Handle) {}

    fn is_mathml_annotation_xml_integration_point(&self, handle: &Handle) -> bool {
        handle.attributes.borrow().iter().any(|attribute| {
            &*attribute.name.local == "encoding"
                && (attribute.value.eq_ignore_ascii_case("text/html")
                    || attribute
                        .value
                        .eq_ignore_ascii_case("application/xhtml+xml"))
        })
    }
}

/// Parses `html` as a browser parses a body fragment.
fn parse(html: &str) -> Recorder {
    parse_fragment(
        Recorder::new(),
        ParseOpts::default(),
        QualName::new(None, ns!(html), local_name!("div")),
        Vec::new(),
        false,
    )
    .one(html)
}

/// Ok once every element a browser builds from `html` and every
/// attribute proved to be on the allowlist; the first thing off it
/// otherwise.
pub fn check(html: &str, own_host: Option<&str>) -> Result<(), String> {
    let recorder = parse(html);
    if recorder.comments.get() > 0 {
        return Err("a comment survived".to_owned());
    }
    let elements = recorder.elements.borrow();
    let (synthetic, parsed) = elements.split_at(SYNTHETIC_ELEMENTS);
    assert_eq!(names(synthetic), ["div", "html"]);
    for element in parsed {
        check_element(element, own_host)?;
    }
    Ok(())
}

/// The elements a browser builds from `html`, each as its name with its
/// attributes in a fixed order and its namespace in front when that is
/// not HTML, so two serializations of one tree compare equal.
pub fn shape(html: &str) -> Vec<String> {
    let recorder = parse(html);
    let elements = recorder.elements.borrow();
    elements
        .iter()
        .skip(SYNTHETIC_ELEMENTS)
        .map(|element| {
            let name = element.name.as_ref().expect("an element");
            let mut words = vec![if name.ns == ns!(html) {
                name.local.to_string()
            } else {
                format!("{}:{}", &*name.ns, name.local)
            }];
            let mut attributes: Vec<String> = element
                .attributes
                .borrow()
                .iter()
                .map(|attribute| format!("{}={}", attribute.name.local, attribute.value))
                .collect();
            attributes.sort();
            words.append(&mut attributes);
            words.join(" ")
        })
        .collect()
}

fn names(elements: &[Handle]) -> Vec<String> {
    elements
        .iter()
        .map(|element| {
            element
                .name
                .as_ref()
                .map_or_else(String::new, |name| name.local.to_string())
        })
        .collect()
}

fn check_element(element: &Node, own_host: Option<&str>) -> Result<(), String> {
    let name = element.name.as_ref().expect("an element");
    let local = &*name.local;
    if name.ns != ns!(html) {
        return Err(format!("<{local}> outside the HTML namespace"));
    }
    if FORBIDDEN_ELEMENTS.contains(&local) || !TAGS.contains(&local) {
        return Err(format!("<{local}> off the allowlist"));
    }
    let attributes = element.attributes.borrow();
    for attribute in attributes.iter() {
        check_attribute(local, &attribute.name.local, &attribute.value, own_host)?;
    }
    if local == "a" {
        let set = |wanted: &str, value: &str| {
            attributes
                .iter()
                .any(|attribute| &*attribute.name.local == wanted && &*attribute.value == value)
        };
        if !set("target", LINK_TARGET) || !set("rel", LINK_REL) {
            return Err("a link without the fixed target and rel".to_owned());
        }
    }
    Ok(())
}

fn check_attribute(
    element: &str,
    attribute: &str,
    value: &str,
    own_host: Option<&str>,
) -> Result<(), String> {
    if attribute.starts_with("on") {
        return Err(format!("<{element} {attribute}> is an event handler"));
    }
    let fixed = element == "a" && matches!(attribute, "target" | "rel");
    let url = URL_ATTRIBUTES.contains(&(element, attribute));
    if !fixed && !url && !GENERIC_ATTRIBUTES.contains(&attribute) {
        return Err(format!("<{element} {attribute}> off the allowlist"));
    }
    if url {
        check_url(element, attribute, value, own_host)?;
    }
    Ok(())
}

fn check_url(
    element: &str,
    attribute: &str,
    value: &str,
    own_host: Option<&str>,
) -> Result<(), String> {
    let place = format!("<{element} {attribute}=\"{value}\">");
    let url = Url::parse(value).map_err(|_| format!("{place} is no absolute URL"))?;
    let schemes = if attribute == "href" {
        LINK_SCHEMES
    } else {
        IMAGE_SCHEMES
    };
    if !schemes.contains(&url.scheme()) {
        return Err(format!("{place} carries a scheme off the list"));
    }
    if url.scheme() == "data" && !url.path().to_ascii_lowercase().starts_with("image/") {
        return Err(format!("{place} is a data URL that is no image"));
    }
    if url.path().starts_with("/api/") {
        return Err(format!("{place} points at the API"));
    }
    if own_host.is_some_and(|host| url.host_str() == Some(host)) {
        return Err(format!("{place} points at the instance"));
    }
    Ok(())
}
