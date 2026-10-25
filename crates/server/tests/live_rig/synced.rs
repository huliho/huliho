// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! An account whose target holds the synced latency corpus. A test
//! leaves every account the same way: the corpus gone, the second
//! connection out and the row removed.

use std::time::Duration;

use super::{Editor, Live, Mail, Target, clear, corpus_size, editor, seed, within};

/// How long the first sync of the latency corpus may take: a floor plus
/// a share per message.
const SYNC_FLOOR: Duration = Duration::from_secs(60);
const SYNC_PER_MESSAGE: Duration = Duration::from_millis(10);

/// An instance whose account on the target holds the latency corpus,
/// synced: the account, the second connection and the inbox's id.
pub struct Synced {
    pub live: Live,
    pub mail: Mail,
    pub editor: Editor,
    pub inbox: String,
}

impl Synced {
    /// The latency corpus seeded on the target and its account added,
    /// once the inbox holds every message.
    pub async fn start(target: Target) -> Self {
        let corpus = corpus_size();
        let live = Live::start().await;
        let mut editor = editor(target).await;
        clear(&mut editor).await;
        seed(&mut editor, 1, corpus).await;
        let mail = live.add(&target.body(), target.is_bridge()).await;
        let patience = SYNC_FLOOR + SYNC_PER_MESSAGE * corpus;
        let inbox = live.wait_synced(&mail, "inbox", patience).await;
        assert!(inbox["totalEmails"].as_u64().unwrap() >= u64::from(corpus));
        let inbox = inbox["id"].as_str().unwrap().to_owned();
        Self {
            live,
            mail,
            editor,
            inbox,
        }
    }

    pub async fn leave(self) {
        leave(&self.live, &self.mail, self.editor).await;
    }
}

/// The account left as it was found: the corpus gone, the editor out,
/// the row removed.
pub async fn leave(live: &Live, mail: &Mail, mut editor: Editor) {
    clear(&mut editor).await;
    within(editor.logout()).await.unwrap();
    live.remove(mail).await;
}
