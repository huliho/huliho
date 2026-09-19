// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The lexer against lines built to slip past it, and the reader around
//! it.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::thread;

use async_imap::imap_proto::Response;
use proptest::prelude::*;
use tokio::io::{AsyncRead, ReadBuf};

use super::lexer::Lexer;
use super::{Guarded, Limit, MAX_NESTING, MAX_RESPONSE_BYTES};

/// Half the stack of a tokio worker thread, so the frames above the
/// parser keep the other half.
const PARSER_STACK: usize = 1024 * 1024;

const LEAF: &str = "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 1 1)";
const ENVELOPE: &str = "(NIL \"s\" ((\"n\" NIL \"a\" \"h\")) NIL NIL NIL NIL NIL NIL NIL)";

fn fetch(body: &str) -> String {
    format!("* 1 FETCH (UID 1 BODYSTRUCTURE {body})\r\n")
}

/// Multiparts around a leaf, the deepest parenthesis `depth` levels in.
fn multiparts(depth: usize) -> String {
    let mut body = LEAF.to_owned();
    for _ in 2..depth {
        body = format!("({body} \"MIXED\")");
    }
    fetch(&body)
}

/// A message inside a multipart inside a message, `levels` times; the
/// address of the innermost envelope sits deepest.
fn messages(levels: usize) -> String {
    let mut body = LEAF.to_owned();
    for _ in 0..levels {
        body = format!(
            "(\"MESSAGE\" \"RFC822\" NIL NIL NIL \"7BIT\" 9 {ENVELOPE} ({body} \"MIXED\") 1)"
        );
    }
    fetch(&body)
}

/// The deepest parenthesis of a line that holds none inside a string.
fn deepest(line: &str) -> usize {
    let mut depth = 0_usize;
    let mut deepest = 0;
    for byte in line.bytes() {
        match byte {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        deepest = deepest.max(depth);
    }
    deepest
}

/// The most message levels whose line stays inside the bound.
fn message_levels() -> usize {
    (1..=MAX_NESTING)
        .take_while(|levels| deepest(&messages(*levels)) <= MAX_NESTING)
        .last()
        .unwrap()
}

fn fed(input: &[u8]) -> Result<(), Limit> {
    Lexer::default().feed(input)
}

/// `opening`, then a line one level past the bound.
fn hides_nothing(opening: &str) {
    let input = format!("{opening}{}", multiparts(MAX_NESTING + 1));
    assert_eq!(fed(input.as_bytes()), Err(Limit::Nesting), "{opening:?}");
}

#[test]
fn a_line_at_the_bound_passes_and_one_level_more_trips() {
    assert_eq!(deepest(&multiparts(MAX_NESTING)), MAX_NESTING);
    assert_eq!(fed(multiparts(MAX_NESTING).as_bytes()), Ok(()));
    assert_eq!(
        fed(multiparts(MAX_NESTING + 1).as_bytes()),
        Err(Limit::Nesting)
    );
    let levels = message_levels();
    assert_eq!(fed(messages(levels).as_bytes()), Ok(()));
    assert_eq!(fed(messages(levels + 1).as_bytes()), Err(Limit::Nesting));
}

#[test]
fn what_the_lexer_admits_the_parser_reads_on_half_a_worker_stack() {
    for line in [multiparts(MAX_NESTING), messages(message_levels())] {
        assert_eq!(fed(line.as_bytes()), Ok(()));
        let parsed = thread::Builder::new()
            .stack_size(PARSER_STACK)
            .spawn(move || Response::from_bytes(line.as_bytes()).is_ok())
            .unwrap()
            .join()
            .unwrap();
        assert!(parsed);
    }
}

#[test]
fn a_parenthesis_inside_a_quoted_string_or_a_literal_does_not_count() {
    let many = "(".repeat(4 * MAX_NESTING);
    let quoted = format!(
        "* 1 FETCH (BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"NAME\" \"a\\\"{many}\\\\\") NIL NIL \"7BIT\" 1 1))\r\n"
    );
    let header = format!("Subject: {many}\"\r\n\r\n");
    let literal = format!(
        "* 1 FETCH (BODY[HEADER.FIELDS (SUBJECT)] {{{}}}\r\n{header})\r\n",
        header.len()
    );
    let mut lexer = Lexer::default();
    for line in [&quoted, &literal, &multiparts(MAX_NESTING)] {
        assert_eq!(lexer.feed(line.as_bytes()), Ok(()), "{line}");
    }
    assert_eq!(
        lexer.feed(multiparts(MAX_NESTING + 1).as_bytes()),
        Err(Limit::Nesting)
    );
}

#[test]
fn the_depth_carries_across_a_literal() {
    let opens = "(".repeat(MAX_NESTING - 1);
    let at_the_bound = format!("* X {opens}{{3}}\r\n)))(");
    assert_eq!(fed(at_the_bound.as_bytes()), Ok(()));
    let past_it = format!("* X {opens}{{3}}\r\n)))((");
    assert_eq!(fed(past_it.as_bytes()), Err(Limit::Nesting));
}

#[test]
fn a_size_in_free_text_hides_nothing_rfc3501_7_1() {
    for opening in [
        "* OK x {100000}\r\n",
        "* ok [BADCHARSET ({100000}\r\n",
        "* NO {100000}\r\n",
        "* BAD {100000}\r\n",
        "* BYE {100000}\r\n",
        "* PREAUTH {100000}\r\n",
        "* OKAY {100000}\r\n",
        "*OK {100000}\r\n",
        "A1 OK {100000}\r\n",
        "A1 NO \"{100000}\r\n",
        "+ {100000}\r\n",
    ] {
        hides_nothing(opening);
    }
}

#[test]
fn a_size_the_parser_would_refuse_is_no_literal_rfc3501_4_3() {
    for opening in [
        "* X {12x}\r\n",
        "* X {12} \r\n",
        "* X {99999999999}\r\n",
        "* X {5}\n",
        "* X {5}\rx",
        "* X {}\r\n",
        "* X {-5}\r\n",
    ] {
        hides_nothing(opening);
    }
}

#[test]
fn an_open_quote_ends_with_its_line() {
    for opening in ["* X \"abc\r\n", "* X \"abc\\\r\n", "* X \"abc\\\\\\\r\n"] {
        hides_nothing(opening);
    }
}

#[test]
fn the_byte_bound_holds_per_response_and_starts_over_after_each() {
    let opening = |size: usize| format!("* X {{{size}}}\r\n");
    let closing = b"\r\n";
    let size = MAX_RESPONSE_BYTES - opening(MAX_RESPONSE_BYTES).len() - closing.len();
    assert_eq!(
        opening(size).len() + size + closing.len(),
        MAX_RESPONSE_BYTES
    );
    let filling = vec![b'('; size + 1];
    let mut lexer = Lexer::default();
    for _ in 0..3 {
        assert_eq!(lexer.feed(opening(size).as_bytes()), Ok(()));
        assert_eq!(lexer.feed(&filling[..size]), Ok(()));
        assert_eq!(lexer.feed(closing), Ok(()));
    }
    assert_eq!(lexer.feed(opening(size + 1).as_bytes()), Ok(()));
    assert_eq!(lexer.feed(&filling), Ok(()));
    assert_eq!(lexer.feed(closing), Err(Limit::Size));
}

#[test]
fn a_line_that_never_ends_passes_the_byte_bound() {
    let mut lexer = Lexer::default();
    let chunk = vec![b'x'; 1024 * 1024];
    let outcome = (0..=MAX_RESPONSE_BYTES / chunk.len())
        .map(|_| lexer.feed(&chunk))
        .find(Result::is_err);
    assert_eq!(outcome, Some(Err(Limit::Size)));
}

/// The bytes the grammar turns on, so random input reaches every state.
fn turning_bytes() -> impl Strategy<Value = Vec<u8>> {
    let alphabet = b"()\"\\{}0123456789\r\n* OKx".to_vec();
    prop::collection::vec(prop::sample::select(alphabet), 0..256)
}

/// One piece of structured data as a server writes it.
#[derive(Debug, Clone)]
enum Item {
    Atom,
    Quoted(String),
    Literal(Vec<u8>),
    List(Vec<Item>),
}

fn item() -> impl Strategy<Value = Item> {
    let leaf = prop_oneof![
        Just(Item::Atom),
        "[a-z(){}\\\\\" ]{0,12}".prop_map(Item::Quoted),
        prop::collection::vec(any::<u8>(), 0..24).prop_map(Item::Literal),
    ];
    leaf.prop_recursive(6, 48, 4, |inner| {
        prop::collection::vec(inner, 0..4).prop_map(Item::List)
    })
}

fn render(item: &Item, out: &mut Vec<u8>) {
    match item {
        Item::Atom => out.extend_from_slice(b"NIL"),
        Item::Quoted(text) => {
            out.push(b'"');
            for byte in text.bytes() {
                if matches!(byte, b'"' | b'\\') {
                    out.push(b'\\');
                }
                out.push(byte);
            }
            out.push(b'"');
        }
        Item::Literal(bytes) => {
            out.extend_from_slice(format!("{{{}}}\r\n", bytes.len()).as_bytes());
            out.extend_from_slice(bytes);
        }
        Item::List(items) => {
            out.push(b'(');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b' ');
                }
                render(item, out);
            }
            out.push(b')');
        }
    }
}

/// The list levels of an item, the only thing the lexer may count.
fn levels(item: &Item) -> usize {
    match item {
        Item::List(items) => 1 + items.iter().map(levels).max().unwrap_or(0),
        _ => 0,
    }
}

proptest! {
    #[test]
    fn chunk_borders_change_nothing(
        input in turning_bytes(),
        cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..8),
    ) {
        let mut whole = Lexer::default();
        let expected = whole.feed(&input);
        let mut cuts: Vec<usize> = cuts.iter().map(|cut| cut.index(input.len() + 1)).collect();
        cuts.sort_unstable();
        let mut chunked = Lexer::default();
        let mut outcome = Ok(());
        let mut start = 0;
        for end in cuts.into_iter().chain([input.len()]) {
            if outcome.is_ok() {
                outcome = chunked.feed(&input[start..end]);
            }
            start = end;
        }
        prop_assert_eq!(outcome, expected);
        if expected.is_ok() {
            prop_assert_eq!(chunked, whole);
        }
    }

    #[test]
    fn the_lexer_counts_the_lists_and_nothing_inside_a_string(
        inner in item(),
        wraps in 0..2 * MAX_NESTING,
    ) {
        let mut item = inner;
        for _ in 0..wraps {
            item = Item::List(vec![item]);
        }
        let mut line = b"* X ".to_vec();
        render(&item, &mut line);
        line.extend_from_slice(b"\r\n");
        let expected = if levels(&item) > MAX_NESTING {
            Err(Limit::Nesting)
        } else {
            Ok(())
        };
        prop_assert_eq!(fed(&line), expected);
    }
}

fn read(guarded: &mut Guarded<&[u8]>, buf: &mut ReadBuf<'_>) -> io::Result<()> {
    let mut context = Context::from_waker(Waker::noop());
    match Pin::new(guarded).poll_read(&mut context, buf) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("a slice is always ready"),
    }
}

fn limit_of(error: &io::Error) -> Option<Limit> {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Limit>())
        .copied()
}

#[test]
fn bytes_inside_the_bounds_pass_through_unchanged() {
    let line = b"* OK ready\r\n";
    let mut guarded = Guarded::new(&line[..]);
    let mut space = [0; 64];
    let mut buf = ReadBuf::new(&mut space);
    read(&mut guarded, &mut buf).unwrap();
    assert_eq!(buf.filled(), line);
}

#[test]
fn a_tripped_guard_withholds_the_chunk_and_fails_every_later_read() {
    let line = multiparts(MAX_NESTING + 1);
    let mut guarded = Guarded::new(line.as_bytes());
    let mut space = vec![0; line.len()];
    let mut buf = ReadBuf::new(&mut space);
    for _ in 0..2 {
        let error = read(&mut guarded, &mut buf).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(limit_of(&error), Some(Limit::Nesting));
        assert!(buf.filled().is_empty());
    }
}
