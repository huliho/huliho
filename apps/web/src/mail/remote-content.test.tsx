// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { SenderPolicy } from "@huliho/core";
import { focusManager, onlineManager } from "@tanstack/react-query";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { AUTHSERV, FAILED, NEWSLETTER_HTML, PASSED, htmlDetail } from "./body-fixtures";
import {
  NEWEST_ID,
  SENDER_POLICIES_ROUTE,
  frame,
  mockCardBox,
  renderCard,
  stubServer,
} from "./card-rig";
import type { Answers } from "./card-rig";

const SENDER = "pieter@blom-installaties.example";
const PROXY = "/api/remote-image?url=";
const BLOCKED = "Remote images are blocked for this sender.";
const LOADED_ONCE = "Images loaded for this message only.";
const ALWAYS = `Always loading images from ${SENDER}.`;
const FAILED_CHECK = "Images stay blocked: this message failed the server’s sender check.";

// The grant the reader gave on a message the server had checked.
const GRANT: SenderPolicy = {
  sender: SENDER,
  key: "remoteContent",
  value: { allow: true, authserv: AUTHSERV },
};

const sessionEnded = vi.fn<() => void>();
vi.mock("../auth/use-session-ended", () => ({
  useSessionEnded: () => sessionEnded,
}));

function bar(): HTMLElement | null {
  return document.querySelector("[data-state]");
}

function buttons(): string[] {
  const found = bar();
  return found === null
    ? []
    : Array.from(found.querySelectorAll("button"), (button) => button.textContent);
}

// Presses a button of the bar the way the keyboard does: focused first.
function press(name: string): void {
  const button = screen.getByRole("button", { name });
  button.focus();
  fireEvent.click(button);
}

mockCardBox();

afterEach(() => {
  cleanup();
  onlineManager.setOnline(true);
  focusManager.setFocused(undefined);
  sessionEnded.mockReset();
});

test("a message with remote images shows the blocked bar; Load once loads them for this view alone", async () => {
  stubServer();
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  const blocked = await frame();
  expect(blocked.srcdoc).toContain("data:image/svg+xml");
  expect(blocked.srcdoc).not.toContain(PROXY);
  expect(bar()?.textContent).toContain(BLOCKED);
  expect(buttons()).toEqual(["Load once", "Always for this sender"]);
  press("Load once");
  const loaded = await frame();
  expect(loaded.srcdoc).toContain(PROXY);
  expect(bar()?.textContent).toContain(LOADED_ONCE);
  expect(buttons()).toEqual([]);
});

test("a choice that leaves hands its focus to the bar and the sentence says the change as a status", async () => {
  stubServer();
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  await frame();
  const strip = bar();
  if (strip === null) {
    throw new Error("no bar");
  }
  // The region stands empty until a choice is taken; the bar reads its sentence when focused.
  expect(within(strip).getByRole("status").textContent).toBe("");
  expect(within(document.body).getByRole("group", { description: BLOCKED })).toBe(strip);
  press("Always for this sender");
  await waitFor(() => {
    expect(within(strip).getByRole("status").textContent).toBe(ALWAYS);
  });
  expect(document.activeElement).toBe(strip);
  press("Stop");
  await waitFor(() => {
    expect(within(strip).getByRole("status").textContent).toBe(BLOCKED);
  });
  expect(document.activeElement).toBe(strip);
  press("Load once");
  expect(within(strip).getByRole("status").textContent).toBe(LOADED_ONCE);
  expect(document.activeElement).toBe(strip);
});

test("Always for this sender writes the grant with the server's pin; Stop takes it back", async () => {
  const seen = stubServer();
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  await frame();
  fireEvent.click(screen.getByRole("button", { name: "Always for this sender" }));
  await waitFor(() => {
    expect(bar()?.textContent).toContain(ALWAYS);
  });
  const written = seen.find((request) => request.method === "PUT");
  expect(written?.url).toBe(`${SENDER_POLICIES_ROUTE}/pieter%40blom-installaties.example`);
  expect(written?.body).toEqual({
    key: "remoteContent",
    value: { allow: true, authserv: AUTHSERV },
  });
  expect((await frame()).srcdoc).toContain(PROXY);
  expect(buttons()).toEqual(["Stop"]);
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  await waitFor(() => {
    expect(bar()?.textContent).toContain(BLOCKED);
  });
  const removed = seen.find((request) => request.method === "DELETE");
  expect(removed?.url).toBe(
    `${SENDER_POLICIES_ROUTE}/pieter%40blom-installaties.example/remoteContent`,
  );
  expect((await frame()).srcdoc).not.toContain(PROXY);
});

test("a standing grant loads a message that passes its pin and holds one that does not", async () => {
  stubServer({ policies: [GRANT] });
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  expect((await frame()).srcdoc).toContain(PROXY);
  expect(bar()?.textContent).toContain(ALWAYS);
  cleanup();
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: FAILED }) },
  });
  expect((await frame()).srcdoc).not.toContain(PROXY);
  expect(bar()?.textContent).toContain(FAILED_CHECK);
  expect(buttons()).toEqual(["Load once"]);
  fireEvent.click(screen.getByRole("button", { name: "Load once" }));
  expect((await frame()).srcdoc).toContain(PROXY);
  expect(bar()?.textContent).toContain(LOADED_ONCE);
  cleanup();
  // A message without the header under a grant that pinned one.
  renderCard({ bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML) } });
  await frame();
  expect(bar()?.textContent).toContain(FAILED_CHECK);
});

test("Always is offered only on a message that would load under its own grant", async () => {
  stubServer();
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: FAILED }) },
  });
  await frame();
  expect(bar()?.textContent).toContain(BLOCKED);
  expect(buttons()).toEqual(["Load once"]);
  cleanup();
  // Without a header on either side the grant is a plain allow on the address.
  renderCard({ bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML) } });
  await frame();
  expect(buttons()).toEqual(["Load once", "Always for this sender"]);
});

test("a message without remote images has no bar; a refused grant puts the policies back and says so", async () => {
  stubServer();
  renderCard({ bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, "<p>plain words</p>") } });
  await frame();
  expect(bar()).toBeNull();
  cleanup();
  stubServer({ writeStatus: 500 });
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  await frame();
  fireEvent.click(screen.getByRole("button", { name: "Always for this sender" }));
  expect(await screen.findByText("Couldn’t save that choice. Try again.")).toBeDefined();
  await waitFor(() => {
    expect(bar()?.textContent).toContain(BLOCKED);
  });
});

test("offline a grant goes out at once, so a refusal takes it back and none stands unwritten", async () => {
  const seen = stubServer({ writeStatus: 503 });
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  await frame();
  onlineManager.setOnline(false);
  fireEvent.click(screen.getByRole("button", { name: "Always for this sender" }));
  expect(await screen.findByText("Couldn’t save that choice. Try again.")).toBeDefined();
  expect(seen.filter((request) => request.method !== "GET")).toHaveLength(1);
  await waitFor(() => {
    expect(bar()?.textContent).toContain(BLOCKED);
  });
});

test("a refused grant comes off at once, even when the list cannot be read again", async () => {
  const answers: Answers = { writeStatus: 500 };
  stubServer(answers);
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  await frame();
  answers.policies = 500;
  press("Always for this sender");
  expect(await screen.findByText("Couldn’t save that choice. Try again.")).toBeDefined();
  await waitFor(() => {
    expect(bar()?.textContent).toContain(BLOCKED);
  });
  expect((await frame()).srcdoc).not.toContain(PROXY);
});

test("the body waits for the policies and a list the server refuses reads as none", async () => {
  stubServer({ policies: 500 });
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  await frame();
  expect(bar()?.textContent).toContain(BLOCKED);
});

test("offline a held body shows under no policy; the standing grant takes hold once the list lands", async () => {
  const seen = stubServer({ policies: [GRANT] });
  onlineManager.setOnline(false);
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  expect((await frame()).srcdoc).not.toContain(PROXY);
  expect(bar()?.textContent).toContain(BLOCKED);
  expect(seen).toEqual([]);
  onlineManager.setOnline(true);
  await waitFor(() => {
    expect(bar()?.textContent).toContain(ALWAYS);
  });
  expect((await frame()).srcdoc).toContain(PROXY);
});

test("a message shown offline keeps showing while the list is read once the network is back", async () => {
  const held = Promise.withResolvers<Response>();
  const asked = vi.fn<() => Promise<Response>>(() => held.promise);
  vi.stubGlobal("fetch", asked);
  onlineManager.setOnline(false);
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  const shown = await frame();
  onlineManager.setOnline(true);
  await waitFor(() => {
    expect(asked).toHaveBeenCalled();
  });
  expect(document.querySelector("iframe")).toBe(shown);
  expect(screen.queryByRole("status", { name: "Loading…" })).toBeNull();
  held.resolve(Response.json([GRANT]));
  await waitFor(() => {
    expect(bar()?.textContent).toContain(ALWAYS);
  });
});

test("a message shown under a list that failed keeps showing while the list is read again", async () => {
  const held = Promise.withResolvers<Response>();
  const asked = vi
    .fn<() => Promise<Response>>()
    .mockResolvedValueOnce(new Response(null, { status: 500 }))
    .mockReturnValue(held.promise);
  vi.stubGlobal("fetch", asked);
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  const shown = await frame();
  // The window takes the focus again, which reads a failed list anew.
  focusManager.setFocused(false);
  focusManager.setFocused(true);
  await waitFor(() => {
    expect(asked).toHaveBeenCalledTimes(2);
  });
  expect(document.querySelector("iframe")).toBe(shown);
  expect(screen.queryByRole("status", { name: "Loading…" })).toBeNull();
  held.resolve(Response.json([]));
});

test("a grant the server answers with a session that ended ends the session and shows no toast", async () => {
  stubServer({ writeStatus: 401 });
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED }) },
  });
  await frame();
  press("Always for this sender");
  await waitFor(() => {
    expect(sessionEnded).toHaveBeenCalledOnce();
  });
  expect(screen.queryByText("Couldn’t save that choice. Try again.")).toBeNull();
});
