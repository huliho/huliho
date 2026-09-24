// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A seeded mailbox of any size behind the mocked proxy: the messages,
// their threads and the shape the proxy serves them in.

// Screenshots must not age, so every message sits before a fixed now.
export const FIXED_NOW = new Date("2026-05-14T10:00:00");
// One message every so many minutes back from now, so a large mailbox
// spans days and weeks.
const STEP_MS = 23 * 60_000;
// Threads come in these sizes, in turn.
const THREAD_SIZES = [1, 3, 1, 2, 1, 4, 1, 1, 2, 1];
const UPSTREAM = "u1";
// Every seventh message carries a flag and every fifth an attachment,
// each at its own place in the run.
const SEVENTH = 7;
const FIFTH = 5;
const FLAGGED_AT = 3;
const ATTACHED_AT = 1;
// A large prime keeps the mailbox seed small.
const SEED_MODULUS = 1_000_003;
const SEED_BASE = 31;

const SENDERS: [string, string][] = [
  ["Mireille Dekker", "mireille@noordwind.example"],
  ["Jonas Verhulst", "jonas@kastanje.example"],
  ["Pieter Blom", "pieter@blom-installaties.example"],
  ["De Koersbrief", "redactie@koersbrief.example"],
  ["Anouk van der Meulen", "anouk@kastanje.example"],
  ["Tomas Lindqvist", "tomas@lindqvist.example"],
  ["Femke Aalders", "femke@kastanje.example"],
  ["Ruben Smit", "ruben@kastanje.example"],
  ["Sven Mulder", "sven@hosting.example"],
  ["Iris Bakker", "iris@familie.example"],
  ["Jeroen Vos", "jeroen@vos-advies.example"],
  ["Kastanje Studio", "nieuws@kastanje.example"],
];

const SUBJECTS = [
  "Serverwissel zaterdagnacht, korte onderbreking",
  "Q3 planning deck + herziene budgetsheet",
  "Offerte badkamerrenovatie, herziene versie",
  "Week 35: rentes, chips en de bouw",
  "Notes from Tuesday's retro",
  "Re: Coffee next week?",
  "Factuur 2026-118, Kastanje Studio",
  "Uren september",
  "Re: Staging-omgeving",
  "Verjaardag oma zondag",
  "Contractverlenging 2027",
  "Nieuwsbrief oktober",
];

const PREVIEWS = [
  "We verhuizen mail-03 tussen 01:00 en 02:30.",
  "Twee bijlagen: de deck is leidend.",
  "Hierbij versie 3 met het tegelwerk erin.",
  "Deze week: de ECB houdt vast, chipexport knelt.",
  "Long one, sorry. TL;DR at the top.",
  "Works for me, Tuesday 14:00.",
  "In de bijlage de factuur voor augustus.",
  "Mijn urenstaat staat in de gedeelde map.",
  "Draait weer; de certificaten waren verlopen.",
  "Om 15:00, jij neemt de taart mee?",
  "Het concept staat klaar, graag je reactie.",
  "Drie nieuwe projecten en een verhuizing.",
];

export interface CorpusEmail {
  id: string;
  threadId: string;
  mailboxId: string;
  receivedAt: string;
  seen: boolean;
  flagged: boolean;
  attachment: boolean;
  from: { name: string; email: string };
  subject: string;
  preview: string;
}

// One mailbox's messages newest first, beside the first message of each
// of its threads in the same order.
interface MailboxList {
  ids: string[];
  exemplars: string[];
}

export interface Corpus {
  emails: Map<string, CorpusEmail>;
  lists: Map<string, MailboxList>;
  // Every thread's messages, oldest first.
  threads: Map<string, string[]>;
}

// A mailbox the corpus fills: the messages it holds, or the synced
// prefix of them while a first sync runs.
export interface CountedMailbox {
  id: string;
  totalEmails: number;
  unreadEmails: number;
  syncedEmails?: number;
}

function pick<Item>(items: readonly Item[], at: number): Item {
  const item = items.at(at % items.length);
  if (item === undefined) {
    throw new Error("an empty list has nothing to pick");
  }
  return item;
}

// A stable number from a mailbox id, so each mailbox starts its sequence elsewhere.
function seedOf(id: string): number {
  let seed = 0;
  for (const char of id) {
    seed = (seed * SEED_BASE + (char.codePointAt(0) ?? 0)) % SEED_MODULUS;
  }
  return seed;
}

function fill(corpus: Corpus, mailbox: CountedMailbox, now: Date): void {
  const count = mailbox.syncedEmails ?? mailbox.totalEmails;
  const seed = seedOf(mailbox.id);
  const list: MailboxList = { ids: [], exemplars: [] };
  let thread = 0;
  let left = 0;
  for (let n = 0; n < count; n += 1) {
    const id = `${mailbox.id}-e${String(n)}`;
    if (left === 0) {
      left = pick(THREAD_SIZES, seed + thread);
      thread += 1;
      list.exemplars.push(id);
    }
    left -= 1;
    const threadId = `${mailbox.id}-t${String(thread)}`;
    const [name, email] = pick(SENDERS, seed + n * SEVENTH);
    corpus.emails.set(id, {
      id,
      threadId,
      mailboxId: mailbox.id,
      receivedAt: new Date(now.getTime() - (n + 1) * STEP_MS).toISOString(),
      seen: n >= mailbox.unreadEmails,
      flagged: n % SEVENTH === FLAGGED_AT,
      attachment: n % FIFTH === ATTACHED_AT,
      from: { name, email },
      subject: pick(SUBJECTS, seed + thread),
      preview: pick(PREVIEWS, seed + n),
    });
    list.ids.push(id);
    corpus.threads.set(threadId, [id, ...(corpus.threads.get(threadId) ?? [])]);
  }
  corpus.lists.set(mailbox.id, list);
}

// The messages of every mailbox that holds some, the same for every run.
export function corpusFor(mailboxes: readonly CountedMailbox[], now = FIXED_NOW): Corpus {
  const corpus: Corpus = { emails: new Map(), lists: new Map(), threads: new Map() };
  for (const mailbox of mailboxes) {
    fill(corpus, mailbox, now);
  }
  return corpus;
}

// A message that arrives at the top of a mailbox after the corpus was
// built, as new mail does; the caller reports it as a change.
export function arrive(corpus: Corpus, mailboxId: string, now = new Date()): CorpusEmail {
  const list = corpus.lists.get(mailboxId) ?? { ids: [], exemplars: [] };
  const n = list.ids.length;
  const id = `${mailboxId}-new${String(n)}`;
  const [name, email] = pick(SENDERS, n);
  const message: CorpusEmail = {
    id,
    threadId: `${mailboxId}-tnew${String(n)}`,
    mailboxId,
    receivedAt: now.toISOString(),
    seen: false,
    flagged: false,
    attachment: false,
    from: { name, email },
    subject: pick(SUBJECTS, n),
    preview: pick(PREVIEWS, n),
  };
  corpus.emails.set(id, message);
  corpus.threads.set(message.threadId, [id]);
  corpus.lists.set(mailboxId, {
    ids: [id, ...list.ids],
    exemplars: [id, ...list.exemplars],
  });
  return message;
}

// The Email object as the proxy would serve it, whole.
export function emailObject(message: CorpusEmail): Record<string, unknown> {
  return {
    id: message.id,
    blobId: message.id,
    threadId: message.threadId,
    mailboxIds: { [message.mailboxId]: true },
    keywords: {
      ...(message.seen ? { $seen: true } : {}),
      ...(message.flagged ? { $flagged: true } : {}),
    },
    size: 2048,
    receivedAt: message.receivedAt,
    messageId: [`${message.id}@example.test`],
    inReplyTo: null,
    references: null,
    sender: null,
    from: [message.from],
    to: [{ name: "Mira", email: "mira@example.com" }],
    cc: null,
    bcc: null,
    replyTo: null,
    subject: message.subject,
    sentAt: null,
    hasAttachment: message.attachment,
    preview: message.preview,
  };
}

export { UPSTREAM };
