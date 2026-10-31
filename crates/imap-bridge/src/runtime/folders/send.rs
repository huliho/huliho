// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The steps of one `Mailbox/set` on one session and what each comes
//! to: an answer per object and the changes for the rows. A name the
//! user typed goes out under the delimiter its level carries, read from
//! one listing when the first name needs it; a mailbox is spelled as
//! the steps before it left it.

use std::collections::HashMap;

use super::delimiters::Delimiters;
use super::failure::{Refused, Stop, failure, kept};
use super::names::{Moved, Names};
use super::step::{Naming, Planned, Step, Under, name_error, sort_order_of};
use super::{Ask, Outcome, Refusal};
use crate::mailboxes::INBOX;
use crate::session::{ListReturn, MailboxName, Place, Session, SessionError};
use crate::store::{MailboxChange, MailboxId, MailboxSnapshot, RoleOverride};

/// One ask as its steps work through it.
pub(super) struct Work {
    gmail: bool,
    /// The creation ids, then the ids of the updates and destroys, in
    /// the order of the ask.
    keys: Vec<String>,
    creates: usize,
    updates: usize,
    /// How each object ended; `None` while it waits.
    results: Vec<Option<Result<(), Refusal>>>,
    /// The mailbox each step names, by its place in the ask: for a
    /// create, the id it takes.
    ids: HashMap<usize, MailboxId>,
    /// The wire name of every mailbox as the steps so far left it.
    names: Names,
    delimiters: Option<Delimiters>,
    pub changes: Vec<MailboxChange>,
    pub refused: Refused,
    /// A RENAME or a DELETE the server took, which moves mail in a
    /// search.
    pub moves_mail: bool,
    /// Whether INBOX is open, so no mailbox a RENAME or a DELETE takes is.
    inbox_open: bool,
}

impl Work {
    pub(super) fn new(ask: &Ask, snapshot: &MailboxSnapshot, planned: &Planned) -> Self {
        let keys: Vec<String> = ask
            .creates
            .iter()
            .map(|create| create.creation_id.clone())
            .chain(ask.updates.iter().map(|update| update.id.clone()))
            .chain(ask.destroys.iter().cloned())
            .collect();
        let mut results = vec![None; keys.len()];
        for (index, refusal) in &planned.refused {
            results[*index] = Some(Err(refusal.clone()));
        }
        Self {
            gmail: snapshot.gmail,
            keys,
            creates: ask.creates.len(),
            updates: ask.updates.len(),
            results,
            ids: planned
                .steps
                .iter()
                .map(|step| (step.index(), step.id().clone()))
                .collect(),
            names: Names::of(&snapshot.rows),
            delimiters: None,
            changes: Vec::new(),
            refused: Refused::Nothing,
            moves_mail: false,
            inbox_open: false,
        }
    }

    /// The wire name the mailbox holds now; one the call never made
    /// names no mailbox.
    fn wire_of(&self, id: &MailboxId) -> Result<String, Stop> {
        self.names.wire(id).map(str::to_owned).ok_or(Stop {
            refusal: Refusal::NotFound,
            stands: true,
        })
    }

    /// INBOX opened read-only ahead of the first RENAME or DELETE. A
    /// server may end the connection whose open mailbox goes, as Dovecot
    /// does; EXAMINE leaves every flag as it is.
    async fn open_inbox<S: Session>(&mut self, session: &mut S) -> Result<(), Stop> {
        if !self.inbox_open {
            match session.examine(INBOX).await {
                Ok(_) | Err(SessionError::Refused) => self.inbox_open = true,
                Err(error) => return Err(failure(&error)),
            }
        }
        Ok(())
    }

    fn end(&mut self, index: usize, result: Result<(), Refusal>) {
        self.results[index] = Some(result);
    }

    /// The updates that send nothing: a role goes to the rows and what
    /// the mailbox holds already is done as it stands. One naming a
    /// mailbox the call was to make and did not ends as it would on the
    /// session.
    pub(super) fn local(&mut self, steps: Vec<Step>) {
        for step in steps {
            let Step::Update {
                index, id, role, ..
            } = step
            else {
                continue;
            };
            let ended = match self.wire_of(&id) {
                Ok(_) => {
                    if let Some(role) = role {
                        self.role(id, role);
                    }
                    Ok(())
                }
                Err(stop) => Err(stop.refusal),
            };
            self.end(index, ended);
        }
    }

    fn role(&mut self, id: MailboxId, role: RoleOverride) {
        let sort_order = sort_order_of(&role);
        self.changes.push(MailboxChange::Role {
            id,
            role,
            sort_order,
        });
    }

    /// Every step on the session, in order. Whether the session still
    /// stands; a failure that ends it leaves the steps after it waiting.
    pub(super) async fn remote<S: Session>(&mut self, session: &mut S, steps: &[Step]) -> bool {
        for step in steps {
            let (index, ended) = match step {
                Step::Create { index, .. } => (*index, self.create(session, step).await),
                Step::Update { index, .. } => (*index, self.update(session, step).await),
                Step::Destroy { index, .. } => (*index, self.destroy(session, step).await),
            };
            let stands = match ended {
                Ok(stands) => {
                    self.end(index, Ok(()));
                    stands
                }
                Err(stop) => {
                    self.end(index, Err(stop.refusal));
                    stop.stands
                }
            };
            if !stands {
                return false;
            }
        }
        true
    }

    /// The name at its place, under the delimiter its level carries. A
    /// parent the call was to make and did not names no mailbox.
    async fn named<S: Session>(
        &mut self,
        session: &mut S,
        naming: &Naming,
    ) -> Result<(MailboxName, Option<char>), Stop> {
        let parent = match &naming.under {
            Under::Top => None,
            Under::Mailbox(id) => Some(self.wire_of(id).map_err(|_| Stop {
                refusal: Refusal::InvalidProperties(vec!["parentId"]),
                stands: true,
            })?),
        };
        let delimiters = self.delimiters(session).await?;
        let delimiter = parent
            .as_deref()
            .map_or(delimiters.top, |wire| delimiters.under(wire));
        let place = Place {
            parent: parent.as_deref(),
            delimiter,
        };
        let name = MailboxName::new(&naming.typed, place).map_err(|error| Stop {
            refusal: name_error(error),
            stands: true,
        })?;
        Ok((name, delimiter))
    }

    /// The delimiters of the listing, read once per ask.
    async fn delimiters<S: Session>(&mut self, session: &mut S) -> Result<&Delimiters, Stop> {
        if self.delimiters.is_none() {
            let listing = session
                .list(ListReturn::default())
                .await
                .map_err(|error| failure(&error))?;
            self.delimiters = Some(Delimiters::of(listing.entries));
        }
        self.delimiters.as_ref().ok_or(Stop {
            refusal: Refusal::ServerUnavailable,
            stands: false,
        })
    }

    /// The command failed: how its object ends, the session noted.
    fn refused(&mut self, error: &SessionError, moves: bool) -> Stop {
        let refused = if moves {
            Refused::Move
        } else {
            Refused::Command
        };
        self.refused = self.refused.max(refused);
        failure(error)
    }

    /// CREATE, then SUBSCRIBE unless the user asked otherwise; a
    /// SUBSCRIBE the server refuses leaves the folder made and
    /// unsubscribed. Whether the session still stands.
    async fn create<S: Session>(&mut self, session: &mut S, step: &Step) -> Result<bool, Stop> {
        let Step::Create {
            id,
            naming,
            role,
            subscribe,
            ..
        } = step
        else {
            return Ok(true);
        };
        let (name, delimiter) = self.named(session, naming).await?;
        if let Err(error) = session.create(&name).await {
            return Err(self.refused(&error, false));
        }
        let mut subscribed = false;
        let mut stands = true;
        if *subscribe {
            match session.subscribe(name.wire()).await {
                Ok(()) => subscribed = true,
                Err(error) => stands = self.refused(&error, false).stands,
            }
        }
        self.names.made(id.clone(), name.wire().to_owned());
        if let Some(delimiters) = &mut self.delimiters {
            delimiters.made(name.wire(), delimiter);
        }
        if let Some(row) = step.made(name.wire(), subscribed, self.gmail) {
            self.changes.push(MailboxChange::Created {
                row,
                role: role.clone(),
            });
        }
        Ok(stands)
    }

    /// RENAME with the subscriptions it leaves behind, then SUBSCRIBE or
    /// UNSUBSCRIBE, then the role, each only where the update asks it.
    async fn update<S: Session>(&mut self, session: &mut S, step: &Step) -> Result<bool, Stop> {
        let Step::Update {
            id,
            rename,
            followers,
            subscribe,
            role,
            ..
        } = step
        else {
            return Ok(true);
        };
        if let Some(naming) = rename {
            let from = self.wire_of(id)?;
            let (to, _) = self.named(session, naming).await?;
            self.open_inbox(session).await?;
            if let Err(error) = session.rename(&from, &to).await {
                return Err(self.refused(&error, true));
            }
            self.moves_mail = true;
            self.changes.push(MailboxChange::Renamed {
                id: id.clone(),
                imap_name: to.wire().to_owned(),
                name: to.leaf().to_owned(),
                parent_id: naming.under.id().cloned(),
            });
            let moved = self.renamed((&from, to.wire()));
            self.follow(session, followers, &moved).await?;
        }
        let wire = self.wire_of(id)?;
        if let Some(subscribed) = *subscribe {
            let sent = if subscribed {
                session.subscribe(&wire).await
            } else {
                session.unsubscribe(&wire).await
            };
            if let Err(error) = sent {
                return Err(self.refused(&error, false));
            }
            self.changes.push(MailboxChange::Subscribed {
                id: id.clone(),
                subscribed,
            });
        }
        if let Some(role) = role {
            self.role(id.clone(), role.clone());
        }
        Ok(true)
    }

    /// Every name a RENAME the server took moved, the names and the
    /// delimiters following it (RFC 3501 section 6.3.5).
    fn renamed(&mut self, (from, to): (&str, &str)) -> Vec<Moved> {
        let delimiter = self.delimiters.as_ref().and_then(|found| found.under(from));
        if let Some(delimiters) = &mut self.delimiters {
            delimiters.renamed((from, to));
        }
        self.names.renamed((from, to), delimiter)
    }

    /// The subscriptions a RENAME left on the old names move to the new
    /// ones, SUBSCRIBE before UNSUBSCRIBE (RFC 9051 section 6.3.6). A
    /// new name the server does not take stays unsubscribed and an old
    /// one it keeps stays behind.
    async fn follow<S: Session>(
        &mut self,
        session: &mut S,
        followers: &[MailboxId],
        moved: &[Moved],
    ) -> Result<(), Stop> {
        for id in followers {
            let Some(moved) = moved.iter().find(|moved| moved.id == *id) else {
                continue;
            };
            match session.subscribe(&moved.to).await {
                Ok(()) => {}
                Err(SessionError::Refused) => self.changes.push(MailboxChange::Subscribed {
                    id: id.clone(),
                    subscribed: false,
                }),
                Err(error) => return Err(failure(&error)),
            }
            kept(session.unsubscribe(&moved.from).await)?;
        }
        Ok(())
    }

    /// DELETE, then UNSUBSCRIBE of a subscribed name, which the server
    /// keeps (RFC 9051 section 6.3.7); the rows leave with their emails
    /// once the DELETE went through.
    async fn destroy<S: Session>(&mut self, session: &mut S, step: &Step) -> Result<bool, Stop> {
        let Step::Destroy { id, subscribed, .. } = step else {
            return Ok(true);
        };
        let wire = self.wire_of(id)?;
        self.open_inbox(session).await?;
        if let Err(error) = session.delete(&wire).await {
            return Err(self.refused(&error, true));
        }
        self.moves_mail = true;
        self.changes
            .push(MailboxChange::Destroyed { id: id.clone() });
        if *subscribed {
            kept(session.unsubscribe(&wire).await)?;
        }
        Ok(true)
    }

    /// The answer per object: the key as written and the id of the
    /// mailbox it stands for; one the work never reached could not be
    /// asked. `before` is the rows as the call found them.
    pub(super) fn outcome(self, before: MailboxSnapshot, new_state: u64) -> Outcome {
        let ids = self.ids;
        let mut ended =
            self.keys
                .into_iter()
                .zip(self.results)
                .enumerate()
                .map(|(index, (key, result))| {
                    let result = result.unwrap_or(Err(Refusal::ServerUnavailable));
                    let id = result
                        .and_then(|()| ids.get(&index).cloned().ok_or(Refusal::ServerUnavailable));
                    (key, id)
                });
        let created = ended.by_ref().take(self.creates).collect();
        let updated = ended.by_ref().take(self.updates).collect();
        let destroyed = ended.collect();
        Outcome::Done {
            before: Box::new(before),
            new_state,
            created,
            updated,
            destroyed,
        }
    }
}
