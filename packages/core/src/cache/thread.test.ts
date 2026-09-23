// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { ACCOUNT, at, email } from "./fake-jmap";
import { MemoryMailStore } from "./memory";
import { readThread } from "./thread";

test("a thread answers its row with the headers the store holds and null when unknown", async () => {
  const store = new MemoryMailStore();
  const held = email("e1", { threadId: "t1", receivedAt: at(1) });
  await store.commit(ACCOUNT, {
    emails: { put: [held] },
    threads: {
      put: [
        {
          id: "t1",
          emailIds: ["e1", "e2"],
          members: {
            e1: { keywords: {}, mailboxIds: { inbox: true } },
            e2: { keywords: { $seen: true }, mailboxIds: { inbox: true } },
          },
        },
      ],
    },
  });
  const detail = await readThread(store, ACCOUNT, "t1");
  expect(detail?.thread.emailIds).toEqual(["e1", "e2"]);
  expect(Object.keys(detail?.emails ?? {})).toEqual(["e1"]);
  expect(detail?.emails["e1"]?.subject).toBe("Message e1");
  expect(await readThread(store, ACCOUNT, "t9")).toBeNull();
});
