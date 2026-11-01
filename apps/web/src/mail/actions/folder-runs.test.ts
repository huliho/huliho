// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, Mailbox, MailboxSetOutcome, MailboxWrite } from "@huliho/core";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { toastManager } from "../../design-system/toast";
import { fixtureCache } from "../fixture-cache";
import { FOLDER_LIMITS } from "../fixtures";
import { closeFolderAsk, settleFolderAsk, useFolderAsk } from "./folder-asks";
import type { ConfirmAsk, NameAsk } from "./folder-asks";
import type { FolderItemInput } from "./folder-items";
import { newFolder, runFolder } from "./folder-runs";
import { MANAGED, managedOf } from "./writable-rig";

// What a server answers a write it took whole.
function written(write: MailboxWrite): MailboxSetOutcome {
  return {
    created: Object.fromEntries(Object.keys(write.create ?? {}).map((key) => [key, "mb-new"])),
    updated: Object.keys(write.update ?? {}),
    destroyed: [...(write.destroy ?? [])],
    notCreated: {},
    notUpdated: {},
    notDestroyed: {},
    changes: { mailboxes: true, windows: [], threads: [] },
  };
}

interface Server {
  cache: MailCache;
  writes: MailboxWrite[];
}

// A cache whose server answers each write as `answer` says and whose
// tree, read again after a write, is `after`.
function served(
  answer: (write: MailboxWrite) => MailboxSetOutcome | Error = written,
  after: readonly Mailbox[] = MANAGED,
): Server {
  const writes: MailboxWrite[] = [];
  const cache: MailCache = {
    ...fixtureCache({}),
    mailboxes: () => Promise.resolve([...after]),
    setMailbox: (_accountId, write) => {
      writes.push(write);
      const outcome = answer(write);
      return outcome instanceof Error ? Promise.reject(outcome) : Promise.resolve(outcome);
    },
  };
  return { cache, writes };
}

// Notes each write in `order`, beside the leaves a test notes there.
function writesInto(server: Server, order: string[]): void {
  const write = server.cache.setMailbox.bind(server.cache);
  server.cache.setMailbox = (accountId, sent) => {
    order.push("write");
    return write(accountId, sent);
  };
}

function inputFor(
  server: Server,
  mailbox: Mailbox,
  extra: Partial<FolderItemInput> = {},
): FolderItemInput {
  return {
    locale: "en",
    cache: server.cache,
    accountId: "acc-1",
    mailbox,
    mailboxes: MANAGED,
    limits: FOLDER_LIMITS,
    shownId: undefined,
    leave: () => undefined,
    ...extra,
  };
}

function toasted(): string[] {
  return vi
    .mocked(toastManager.add)
    .mock.calls.map(([options]) =>
      typeof options.description === "string" ? options.description : "",
    );
}

// A server that refuses a create's name.
function refusing(write: MailboxWrite): MailboxSetOutcome {
  return {
    ...written(write),
    created: {},
    notCreated: { folder: { type: "invalidProperties", properties: ["name"] } },
  };
}

// A server that refuses a destroy of the empty folder, since it holds mail.
function holdingMail(write: MailboxWrite): MailboxSetOutcome {
  return {
    ...written(write),
    destroyed: [],
    notDestroyed: { "mb-empty": { type: "mailboxHasEmail", properties: [] } },
  };
}

// A tree the store cannot read.
function unread(): Promise<Mailbox[]> {
  return Promise.reject(new Error("unavailable"));
}

function asked() {
  return renderHook(() => useFolderAsk()).result.current.ask;
}

function nameAsked(): NameAsk {
  const ask = asked();
  if (ask === null || !("write" in ask)) {
    throw new Error("no name dialog was asked");
  }
  return ask;
}

function confirmAsked(): ConfirmAsk {
  const ask = asked();
  if (ask === null || "write" in ask) {
    throw new Error("no confirm was asked");
  }
  return ask;
}

// A folder under the top that holds nothing and nothing under it.
const EMPTY_FOLDER: Mailbox = {
  ...managedOf("mb-facturen"),
  id: "mb-empty",
  name: "Leeg",
  totalEmails: 0,
};

afterEach(() => {
  act(() => {
    closeFolderAsk();
    settleFolderAsk();
  });
  cleanup();
  vi.restoreAllMocks();
});

test("a hide takes the folder and every subscribed folder under it, the mailbox shown leaving first", async () => {
  vi.spyOn(toastManager, "add");
  const server = served();
  const order: string[] = [];
  // A role mailbox under the folder stays, since the tree draws it on its own.
  const archive = { ...managedOf("mb-archive"), parentId: "mb-verbouwing" };
  const input = inputFor(server, managedOf("mb-verbouwing"), {
    mailboxes: [...MANAGED.filter((row) => row.id !== "mb-archive"), archive],
    shownId: "mb-offertes",
    leave: () => order.push("leave"),
  });
  writesInto(server, order);
  runFolder("hide", input);
  await vi.waitFor(() => {
    expect(toasted()).toEqual(["Folder hidden."]);
  });
  expect(order).toEqual(["leave", "write"]);
  expect(server.writes).toEqual([
    {
      update: { "mb-verbouwing": { isSubscribed: false }, "mb-offertes": { isSubscribed: false } },
    },
  ]);
});

test("a hide of the mailbox shown, drawn only above a subscribed folder, leaves first and unsubscribes that folder alone", async () => {
  vi.spyOn(toastManager, "add");
  const server = served();
  const order: string[] = [];
  const parent = { ...managedOf("mb-verbouwing"), isSubscribed: false };
  const input = inputFor(server, parent, {
    mailboxes: MANAGED.map((row) => (row.id === parent.id ? parent : row)),
    shownId: parent.id,
    leave: () => order.push("leave"),
  });
  writesInto(server, order);
  runFolder("hide", input);
  await vi.waitFor(() => {
    expect(toasted()).toEqual(["Folder hidden."]);
  });
  expect(order).toEqual(["leave", "write"]);
  expect(server.writes).toEqual([{ update: { "mb-offertes": { isSubscribed: false } } }]);
});

test("a delete of a folder with subfolders is refused at once, and so is one offline", async () => {
  vi.spyOn(toastManager, "add");
  const server = served();
  runFolder("delete", inputFor(server, managedOf("mb-verbouwing")));
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  runFolder("delete", inputFor(server, managedOf("mb-offertes")));
  runFolder("hide", inputFor(server, managedOf("mb-offertes")));
  await vi.waitFor(() => {
    expect(toasted()).toEqual([
      "Delete its subfolders first.",
      "Changes need a connection.",
      "Changes need a connection.",
    ]);
  });
  expect(server.writes).toEqual([]);
});

test("an empty folder goes at once with the mail argument off, the mailbox shown leaving first; one with mail asks first", async () => {
  vi.spyOn(toastManager, "add");
  const server = served();
  const order: string[] = [];
  writesInto(server, order);
  const shown = { mailboxes: [...MANAGED, EMPTY_FOLDER], shownId: "mb-empty" };
  runFolder(
    "delete",
    inputFor(server, EMPTY_FOLDER, { ...shown, leave: () => order.push("leave") }),
  );
  await vi.waitFor(() => {
    expect(toasted()).toEqual(["Folder deleted."]);
  });
  expect(order).toEqual(["leave", "write"]);
  expect(server.writes).toEqual([{ destroy: ["mb-empty"], onDestroyRemoveEmails: false }]);
  runFolder("delete", inputFor(server, managedOf("mb-offertes")));
  const ask = confirmAsked();
  expect(ask).toMatchObject({ kind: "delete", mailbox: { id: "mb-offertes", totalEmails: 5 } });
  expect(server.writes).toHaveLength(1);
  act(() => {
    ask.confirm();
  });
  await vi.waitFor(() => {
    expect(server.writes).toHaveLength(2);
  });
  expect(server.writes[1]).toEqual({ destroy: ["mb-offertes"], onDestroyRemoveEmails: true });
  expect(order).toEqual(["leave", "write", "write"]);
});

test("a folder that took mail since the tree was read asks the confirm with the count the store holds", async () => {
  vi.spyOn(toastManager, "add");
  const holding = { ...EMPTY_FOLDER, totalEmails: 3 };
  const server = served(holdingMail, [...MANAGED, holding]);
  runFolder("delete", inputFor(server, EMPTY_FOLDER, { mailboxes: [...MANAGED, EMPTY_FOLDER] }));
  await vi.waitFor(() => {
    expect(asked()).toMatchObject({ kind: "delete", mailbox: { totalEmails: 3 } });
  });
  expect(toasted()).toEqual([]);
});

test("a new folder states its subscription: none where the account subscribes nothing, so the tree keeps every mailbox", async () => {
  const server = served();
  const unsubscribed = MANAGED.map((row) => Object.assign({}, row, { isSubscribed: false }));
  newFolder(
    { cache: server.cache, accountId: "acc-1", mailboxes: unsubscribed, limits: FOLDER_LIMITS },
    null,
  );
  await nameAsked().write("Projecten");
  expect(server.writes).toEqual([
    { create: { folder: { name: "Projecten", parentId: null, isSubscribed: false } } },
  ]);
});

test("a new folder goes out as a create; a name the server refuses is taken where the tree now holds it", async () => {
  vi.spyOn(toastManager, "add");
  const server = served();
  newFolder(
    { cache: server.cache, accountId: "acc-1", mailboxes: MANAGED, limits: FOLDER_LIMITS },
    null,
  );
  const ask = nameAsked();
  expect(ask).toMatchObject({ kind: "create", name: "", place: null });
  expect(ask.taken).toEqual(expect.arrayContaining(["Inbox", "Facturen", "Verbouwing"]));
  await expect(ask.write("Projecten")).resolves.toBeNull();
  // A server may make a folder unsubscribed by default, so the create says it.
  expect(server.writes).toEqual([
    { create: { folder: { name: "Projecten", parentId: null, isSubscribed: true } } },
  ]);
  expect(toasted()).toEqual(["Folder created."]);
  const elsewhere = { ...EMPTY_FOLDER, id: "mb-other", name: "Projecten", parentId: null };
  const taken = served(refusing, [...MANAGED, elsewhere]);
  newFolder(
    { cache: taken.cache, accountId: "acc-1", mailboxes: MANAGED, limits: FOLDER_LIMITS },
    null,
  );
  await expect(nameAsked().write("Projecten")).resolves.toBe("taken");
  const refused = served(refusing);
  newFolder(
    { cache: refused.cache, accountId: "acc-1", mailboxes: MANAGED, limits: FOLDER_LIMITS },
    null,
  );
  await expect(nameAsked().write("a/b")).resolves.toBe("name");
});

test("a refused name is taken where the server keeps the sibling's name decomposed", async () => {
  const composed = "Café".normalize("NFC");
  const sibling = {
    ...EMPTY_FOLDER,
    id: "mb-other",
    name: composed.normalize("NFD"),
    parentId: null,
  };
  const server = served(refusing, [...MANAGED, sibling]);
  newFolder(
    { cache: server.cache, accountId: "acc-1", mailboxes: MANAGED, limits: FOLDER_LIMITS },
    null,
  );
  await expect(nameAsked().write(composed)).resolves.toBe("taken");
});

test("a tree read that fails after a refusal leaves its cause in the console and never the name; the write reads as refused", async () => {
  vi.spyOn(toastManager, "add");
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const create = served(refusing);
  create.cache.mailboxes = unread;
  newFolder(
    { cache: create.cache, accountId: "acc-1", mailboxes: MANAGED, limits: FOLDER_LIMITS },
    null,
  );
  await expect(nameAsked().write("Geheim")).resolves.toBe("name");
  const destroy = served(holdingMail);
  destroy.cache.mailboxes = unread;
  runFolder("delete", inputFor(destroy, EMPTY_FOLDER, { mailboxes: [...MANAGED, EMPTY_FOLDER] }));
  await vi.waitFor(() => {
    expect(toasted()).toEqual(["The server refused. Try again."]);
  });
  expect(logged.mock.calls).toEqual([
    ["mail: a folder tree read failed", "unavailable"],
    ["mail: a folder tree read failed", "unavailable"],
  ]);
});

test("a subfolder names its parent; a rename keeps its place and sends nothing for the same name", async () => {
  const server = served();
  runFolder("subfolder", inputFor(server, managedOf("mb-verbouwing")));
  expect(nameAsked()).toMatchObject({ kind: "create", place: "Verbouwing", taken: ["Offertes"] });
  await nameAsked().write("Tekeningen");
  expect(server.writes.at(-1)).toEqual({
    create: { folder: { name: "Tekeningen", parentId: "mb-verbouwing", isSubscribed: true } },
  });
  runFolder("rename", inputFor(server, managedOf("mb-offertes")));
  const ask = nameAsked();
  expect(ask).toMatchObject({ kind: "rename", name: "Offertes", place: "Verbouwing", taken: [] });
  await expect(ask.write("Offertes")).resolves.toBeNull();
  expect(server.writes).toHaveLength(1);
  await ask.write("Offertes 2026");
  expect(server.writes.at(-1)).toEqual({ update: { "mb-offertes": { name: "Offertes 2026" } } });
});

test("a rename of a folder the server keeps decomposed sends nothing for its name untouched", async () => {
  const server = served();
  const decomposed = { ...managedOf("mb-offertes"), name: "Café".normalize("NFD") };
  runFolder("rename", inputFor(server, decomposed));
  await expect(nameAsked().write("Café".normalize("NFC"))).resolves.toBeNull();
  expect(server.writes).toEqual([]);
});

test("a write that fails whole reads as the server's refusal and names no folder in the console", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const server = served(() => new Error("unavailable"));
  runFolder("rename", inputFor(server, managedOf("mb-offertes")));
  await expect(nameAsked().write("Geheim")).resolves.toBe("server");
  expect(logged.mock.calls.flat().join(" ")).not.toContain("Geheim");
});
