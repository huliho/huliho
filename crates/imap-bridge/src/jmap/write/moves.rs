// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailboxes of an `Email/set` on one folder. A Gmail label is a
//! STORE of X-GM-LABELS on the store the row lies in. A move is `UID
//! MOVE` where the server offers it (RFC 6851), else on a folder account
//! `UID COPY` and an expunge of what it copied (RFC 4315); the new UIDs
//! come from `COPYUID`. A move a Gmail account makes between its stores
//! is `UID MOVE` alone; one into All Mail has the labels of its patch
//! stored there afterwards (`restore`). A destroy stores `\Deleted` and
//! expunges those messages alone. A row whose message the bridge cannot
//! follow is lost to the write. On a Gmail account it waits for the
//! pass, which claims it where the message turned up; elsewhere it
//! leaves the store and the next sync brings the message where it lies.

use std::collections::HashMap;

use super::super::set::SetError;
use super::keywords;
use super::restore::Restore;
use super::{Member, Work, failure};
use crate::gmail;
use crate::session::{
    MAX_STORE_UIDS, MoveAsk, Moved, Session, SessionError, StoreAsk, StoreItem, Writable,
    label_bytes,
};
use crate::store::{GoneRow, LabelChange, MailboxId, MailboxRow};

/// The flag an expunge stores, which the folder has to keep.
const DELETED: &str = "\\Deleted";

/// The labels a message gains and loses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Relabel {
    pub add: Vec<MailboxId>,
    pub remove: Vec<MailboxId>,
}

/// The messages of one Gmail store whose labels change the same way.
pub(super) struct LabelGroup {
    pub relabel: Relabel,
    pub members: Vec<Member>,
}

/// The messages of one folder that go to the same folder, with the
/// labels they gain and lose there when it is All Mail.
pub(super) struct MoveGroup {
    pub to: MailboxId,
    pub then: Option<Relabel>,
    pub members: Vec<Member>,
}

/// Puts the member with the others whose labels change the same way.
pub(super) fn join_labels(groups: &mut Vec<LabelGroup>, relabel: Relabel, member: Member) {
    match groups.iter_mut().find(|group| group.relabel == relabel) {
        Some(group) => group.members.push(member),
        None => groups.push(LabelGroup {
            relabel,
            members: vec![member],
        }),
    }
}

/// Puts the member with the others that go to the same folder the same
/// way.
pub(super) fn join_move(
    groups: &mut Vec<MoveGroup>,
    (to, then): (MailboxId, Option<Relabel>),
    member: Member,
) {
    match groups
        .iter_mut()
        .find(|group| group.to == to && group.then == then)
    {
        Some(group) => group.members.push(member),
        None => groups.push(MoveGroup {
            to,
            then,
            members: vec![member],
        }),
    }
}

/// The label groups of one selected Gmail store. Whether the session
/// still stands.
pub(super) async fn labeled<S: Session>(
    session: &mut S,
    work: &mut Work,
    folder: &MailboxId,
    groups: Vec<LabelGroup>,
) -> bool {
    for group in groups {
        for members in work.standing(&group.members).chunks(MAX_STORE_UIDS) {
            let uids: Vec<u32> = members.iter().map(|member| member.uid).collect();
            match relabeled(session, work, &group.relabel, &uids).await {
                Ok(()) => {
                    for member in members {
                        work.done(member);
                        work.stored.labels.push(LabelChange {
                            id: member.id.clone(),
                            folder: folder.clone(),
                            uid: member.uid,
                            add: group.relabel.add.clone(),
                            remove: group.relabel.remove.clone(),
                        });
                    }
                }
                Err(error) => {
                    let (ends, stands) = failure(&error);
                    work.settle(members, &ends);
                    if !stands {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// The label STOREs of one set: what the messages gain, then what they
/// lose, each label as Gmail spells it, in runs that fit a command line.
pub(super) async fn relabeled<S: Session>(
    session: &mut S,
    work: &Work,
    relabel: &Relabel,
    uids: &[u32],
) -> Result<(), SessionError> {
    for (mailboxes, add) in [(&relabel.add, true), (&relabel.remove, false)] {
        let labels = labels_of(&work.rows, mailboxes);
        for labels in keywords::runs(&labels, width) {
            let ask = StoreAsk {
                uids,
                flags: labels,
                add,
                item: StoreItem::Labels,
            };
            session.uid_store(&ask).await?;
        }
    }
    Ok(())
}

/// The labels of these mailboxes as a STORE writes them.
fn labels_of(rows: &HashMap<MailboxId, MailboxRow>, mailboxes: &[MailboxId]) -> Vec<String> {
    mailboxes
        .iter()
        .filter_map(|mailbox| rows.get(mailbox)?.facts.gmail_label.as_deref())
        .map(|label| gmail::written(label).to_owned())
        .collect()
}

/// The width of a label on the command line. A label the session cannot
/// send is kept out when the update is planned, so none is measured
/// here.
fn width(label: &str) -> usize {
    label_bytes(label).unwrap_or_default()
}

/// Whether every label of the relabel fits a STORE run on its own, so
/// each run that carries one stays on one command line. The names come
/// from the server's LIST, so one too wide is rare.
pub(super) fn sendable(rows: &HashMap<MailboxId, MailboxRow>, relabel: &Relabel) -> bool {
    [&relabel.add, &relabel.remove]
        .into_iter()
        .flat_map(|mailboxes| labels_of(rows, mailboxes))
        .all(|label| label_bytes(&label).is_some_and(keywords::fits))
}

/// The move groups of one selected folder: `UID MOVE` where the server
/// offers it, else a copy and an expunge of the same messages, which
/// needs the folder to keep `\Deleted`. A Gmail account never reaches
/// the copy: its moves are settled before the folder is selected.
/// Whether the session still stands.
pub(super) async fn moved<S: Session>(
    session: &mut S,
    work: &mut Work,
    (folder, writable): (&MailboxId, &Writable),
    groups: Vec<MoveGroup>,
) -> bool {
    let copies = !work.moving.move_command;
    for group in groups {
        if copies && !writable.keeps(DELETED) {
            work.settle(&group.members, &SetError::Forbidden);
            continue;
        }
        let Some(target) = work.wire_name(&group.to).map(str::to_owned) else {
            work.settle(&group.members, &SetError::ServerUnavailable);
            continue;
        };
        for members in work.standing(&group.members).chunks(MAX_STORE_UIDS) {
            let uids: Vec<u32> = members.iter().map(|member| member.uid).collect();
            let ask = MoveAsk {
                uids: &uids,
                target: &target,
            };
            let result = if copies {
                copied(session, &ask).await
            } else {
                session.uid_move(&ask).await
            };
            match result {
                Ok(moved) => landed(work, (folder, &group), members, moved.as_ref()),
                Err(error) => {
                    let (ends, stands) = failure(&error);
                    work.settle(members, &ends);
                    if !stands {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `UID COPY`, then an expunge of the messages it copied: the source
/// UIDs `COPYUID` pairs, or the whole set where the answer names none.
/// When the expunge fails the copies stand beside the messages.
async fn copied<S: Session>(
    session: &mut S,
    ask: &MoveAsk<'_>,
) -> Result<Option<Moved>, SessionError> {
    let moved = session.uid_copy(ask).await?;
    let copied: Vec<u32> = moved.as_ref().map_or_else(
        || ask.uids.to_vec(),
        |moved| moved.uids.iter().map(|(from, _)| *from).collect(),
    );
    if !copied.is_empty() {
        session.uid_expunge(&copied).await?;
    }
    Ok(moved)
}

/// The rows of a set the server moved. A message with a pair is written
/// to the target under its new UID, or waits for its labels where the
/// target is All Mail; one without a pair named no message and is not
/// found, as is every message of an answer without `COPYUID` from a
/// server with UIDPLUS (RFC 4315 section 3). Without UIDPLUS the bridge
/// cannot follow the messages that went. The rows of both are lost to
/// the write.
fn landed(
    work: &mut Work,
    (folder, group): (&MailboxId, &MoveGroup),
    members: &[Member],
    moved: Option<&Moved>,
) {
    for member in members {
        let lost = GoneRow {
            id: member.id.clone(),
            folder: folder.clone(),
            uid: member.uid,
        };
        let Some(moved) = moved else {
            work.stored.lost.push(lost);
            let failed = if work.moving.uidplus {
                Some(SetError::NotFound)
            } else {
                group.then.as_ref().map(|_| SetError::ServerFail)
            };
            match failed {
                Some(error) => work.settle(std::slice::from_ref(member), &error),
                None => work.done(member),
            }
            continue;
        };
        let pair = moved.uids.iter().find(|(from, _)| *from == member.uid);
        let Some((_, uid)) = pair.copied() else {
            work.stored.lost.push(lost);
            work.settle(std::slice::from_ref(member), &SetError::NotFound);
            continue;
        };
        let restore = Restore {
            member: member.clone(),
            from: folder.clone(),
            to: group.to.clone(),
            uid_validity: moved.uid_validity,
            uid,
            relabel: group.then.clone().unwrap_or_default(),
        };
        if group.then.is_some() {
            work.restores.push(restore);
        } else {
            work.done(member);
            work.stored.moves.push(restore.change(None));
        }
    }
}

/// The destroys of one selected folder: `\Deleted` and `UID EXPUNGE` of
/// those messages, where the folder keeps `\Deleted`. Whether the
/// session still stands.
pub(super) async fn destroyed<S: Session>(
    session: &mut S,
    work: &mut Work,
    (folder, writable): (&MailboxId, &Writable),
    members: &[Member],
) -> bool {
    if !writable.keeps(DELETED) {
        work.settle(members, &SetError::Forbidden);
        return true;
    }
    for members in work.standing(members).chunks(MAX_STORE_UIDS) {
        let uids: Vec<u32> = members.iter().map(|member| member.uid).collect();
        match session.uid_expunge(&uids).await {
            Ok(()) => {
                for member in members {
                    work.done(member);
                    work.stored.gone.push(GoneRow {
                        id: member.id.clone(),
                        folder: folder.clone(),
                        uid: member.uid,
                    });
                }
            }
            Err(error) => {
                let (ends, stands) = failure(&error);
                work.settle(members, &ends);
                if !stands {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jmap::set::MAX_UPDATE_KEYWORD_BYTES;

    /// Two bytes the quoting escapes.
    const PAIR: &str = "\\\"";

    /// Pairs enough that a few labels fill a run.
    const ESCAPED_PAIRS: usize = 100;

    /// More labels than one run holds.
    const MANY: usize = 40;

    /// A user label whose every byte but the first is escaped on the
    /// line.
    fn label(n: usize) -> String {
        format!("{n}{}", PAIR.repeat(ESCAPED_PAIRS))
    }

    #[test]
    fn labels_past_one_command_line_go_out_in_runs_that_fit_quoted_rfc9051_4_3() {
        let labels: Vec<String> = (0..MANY).map(label).collect();
        let found = keywords::runs(&labels, width);
        assert!(found.len() > 1);
        for run in &found {
            let bytes: usize = run.iter().map(|label| width(label) + 1).sum();
            assert!(bytes <= MAX_UPDATE_KEYWORD_BYTES, "{bytes}");
        }
        assert_eq!(found.concat(), labels);
        let quotes = "\"\"".len();
        assert_eq!(
            width(&labels[0]),
            labels[0].len() + quotes + PAIR.len() * ESCAPED_PAIRS
        );
        assert_eq!(width("\\Inbox"), "\\Inbox".len());
        assert_eq!(width("Work"), "\"Work\"".len());
    }
}
