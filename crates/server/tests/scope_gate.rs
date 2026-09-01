// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Proof that the scope capability cannot be forged outside the resolver.

#[test]
fn a_scope_is_only_obtainable_through_the_resolver() {
    trybuild::TestCases::new().compile_fail("tests/scope_gate/forge.rs");
}
