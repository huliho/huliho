// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { ACCOUNT } from "@huliho/core/testing";
import { afterEach, beforeEach, expect, test } from "vitest";

import { attached, beginRig, coordinate, endRig, posts, serve, settled } from "./coordinator-rig";
import type { CacheMessage } from "./messages";

beforeEach(beginRig);
afterEach(endRig);

// The least name size RFC 8621 section 1.3.1 lets a server name.
const NAME_BYTES = 100;

test("a folder write lands before it answers and every tab hears the tree moved", async () => {
  const { posted, tab } = await attached(1);
  const before = posted.length;
  const answer = await tab.setMailbox(ACCOUNT, {
    create: { folder: { name: "Facturen", parentId: null } },
  });
  expect(answer).toMatchObject({ ok: true, value: { notCreated: {} } });
  const id = answer.ok ? answer.value.created["folder"] : undefined;
  expect(posted.slice(before)).toEqual([
    { kind: "changed", accountId: ACCOUNT, mailboxes: true, windows: [], threads: [] },
  ]);
  const tree = await tab.mailboxes(ACCOUNT);
  expect(tree.ok ? tree.value.find((row) => row.id === id)?.name : null).toBe("Facturen");
});

test("a write the server refuses moves nothing and tells the tabs nothing", async () => {
  const { server, posted, tab } = await attached(1);
  server.folderRefusal = "forbidden";
  const before = posted.length;
  const answer = await tab.setMailbox(ACCOUNT, { destroy: ["archive"] });
  expect(answer).toMatchObject({
    ok: true,
    value: { destroyed: [], notDestroyed: { archive: { type: "forbidden", properties: [] } } },
  });
  expect(posted.slice(before)).toEqual([]);
});

test("a write past the session's objects goes nowhere and crosses as the limit", async () => {
  const { server, tab } = await attached(1);
  const sent = server.posted().length;
  const destroy = Array.from(
    { length: server.maxObjectsInSet + 1 },
    (_, index) => `m${String(index)}`,
  );
  const answer = await tab.setMailbox(ACCOUNT, { destroy });
  expect(answer).toMatchObject({ ok: false, failure: { code: "limit", limit: "maxObjectsInSet" } });
  expect(server.posted()).toHaveLength(sent);
});

test("the folder limits come from the account's mail capability", async () => {
  const server = serve(1);
  server.topLevel = false;
  server.maxSizeMailboxName = NAME_BYTES;
  const posted: CacheMessage[] = [];
  const tab = coordinate(posted).api();
  await tab.persisted();
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null, strict: false });
  await settled(posts(posted, 1));
  await expect(tab.mailboxLimits(ACCOUNT)).resolves.toEqual({
    ok: true,
    value: { mayCreateTopLevelMailbox: false, maxSizeMailboxName: NAME_BYTES },
  });
});
