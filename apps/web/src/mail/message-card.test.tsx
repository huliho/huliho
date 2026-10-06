// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { BodyDetail } from "@huliho/core";
import { onlineManager } from "@tanstack/react-query";
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { forgetLinkKey, linkKey } from "../open/link-key";
import {
  LOGO,
  LOGO_IMAGE,
  NEWSLETTER_HTML,
  REPLY_TEXT,
  attachmentDetail,
  complexDetail,
  htmlDetail,
  textDetail,
} from "./body-fixtures";
import {
  NEWEST_ID,
  frame,
  mockCardBox,
  newest,
  planned,
  renderCard,
  stubServer,
  unread,
} from "./card-rig";
import { PLAIN_LINES_MAX } from "./plain-text";

const DECLARES_DARK =
  "<style>p { color: rgb(1, 1, 1) } @media (prefers-color-scheme: dark) { p { color: rgb(250, 250, 250) } }</style><p>x</p>";

mockCardBox();

afterEach(() => {
  cleanup();
  onlineManager.setOnline(true);
});

test("an HTML message renders in the sandboxed frame, titled by who wrote and what about, under the hairline", async () => {
  stubServer();
  renderCard({
    bodies: {
      [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML + LOGO_IMAGE, { attachments: [LOGO] }),
    },
  });
  expect(await screen.findByRole("status", { name: "Loading…" })).toBeDefined();
  const shown = await frame();
  expect(shown.title).toBe(
    "Message from Pieter Blom about Re: Offerte badkamerrenovatie, herziene versie",
  );
  expect(shown.getAttribute("sandbox")).toBe(
    "allow-same-origin allow-popups allow-popups-to-escape-sandbox",
  );
  expect(shown.srcdoc).toContain("Week 35: rentes, chips en de bouw");
  expect(shown.srcdoc).toContain("/open#k=");
  expect(shown.srcdoc).not.toContain("https://cdn.example/hero.png");
  expect(shown.srcdoc).toContain("/api/jmap/acc-1/download/u1/e-3-3/logo.png?type=image%2Fpng");
  expect(shown.hasAttribute("data-canvas")).toBe(false);
  expect(screen.queryByRole("status", { name: "Loading…" })).toBeNull();
});

test("a plain message renders as text with its quotes; one without text and one too complex say so", async () => {
  stubServer();
  renderCard({ bodies: { [NEWEST_ID]: textDetail(NEWEST_ID, REPLY_TEXT) } });
  const quoted = await screen.findByText(/Hierbij de eerste versie van de offerte/);
  expect(quoted.closest("[data-depth]")?.getAttribute("data-depth")).toBe("1");
  expect(
    screen
      .getByText(/Kunnen jullie een offerte maken/)
      .closest("[data-depth]")
      ?.getAttribute("data-depth"),
  ).toBe("2");
  expect(
    screen.getByRole("link", { name: "https://www.blom-installaties.example/offertes" }),
  ).toBeDefined();
  expect(screen.queryByTitle(/^Message from/)).toBeNull();
  cleanup();
  renderCard({ bodies: { [NEWEST_ID]: attachmentDetail(NEWEST_ID) } });
  expect(await screen.findByText("This message has no text.")).toBeDefined();
  cleanup();
  renderCard({ bodies: { [NEWEST_ID]: complexDetail(NEWEST_ID) } });
  expect(await screen.findByText("This message is too complex to show here.")).toBeDefined();
});

test("a link carries the device's key and none once the key is forgotten, so the open route asks first", async () => {
  stubServer();
  const address = "https://shop.example.test/sale";
  renderCard({ bodies: { [NEWEST_ID]: textDetail(NEWEST_ID, `see ${address}`) } });
  const link = await screen.findByRole("link", { name: address });
  expect(link.getAttribute("href")).toContain(`#k=${linkKey()}&`);
  act(() => {
    forgetLinkKey();
  });
  expect(screen.getByRole("link", { name: address }).getAttribute("href")).toContain("#k=&");
});

test("the body loads, fails with Try again, says so offline and says when the message is gone", async () => {
  stubServer();
  renderCard({ bodies: { [NEWEST_ID]: "never" } });
  expect(await screen.findByRole("status", { name: "Loading…" })).toBeDefined();
  cleanup();
  const body = vi
    .fn<() => Promise<BodyDetail | null>>()
    .mockRejectedValueOnce(new JmapError("unavailable"))
    .mockResolvedValue(textDetail(NEWEST_ID, "landed after all"));
  const { cache } = renderCard();
  cache.body = body;
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain(
    "Couldn’t load this message. Your mail is safe; nothing was lost.",
  );
  const retry = within(alert).getByRole("button", { name: "Try again" });
  retry.focus();
  fireEvent.click(retry);
  const landed = await screen.findByText("landed after all");
  // The button left with the fault; the body's box holds its focus.
  expect(document.activeElement).toBe(landed.closest("[tabindex]"));
  cleanup();
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  renderCard({ bodies: { [NEWEST_ID]: new JmapError("unavailable") } });
  const offline = await screen.findByText("Offline: this message isn’t stored on this device.");
  expect(offline.closest("[role]")?.getAttribute("role")).toBe("status");
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  cleanup();
  renderCard({ bodies: { [NEWEST_ID]: null } });
  expect(await screen.findByText("This message isn’t on the server anymore.")).toBeDefined();
});

test("a card marks its message read as it opens unread, once per opening; a read card asks nothing", async () => {
  stubServer();
  const { cache } = renderCard({ message: planned(unread()) });
  await waitFor(() => {
    expect(cache.mutations).toHaveLength(1);
  });
  const asked = { type: "Email", id: NEWEST_ID, patch: { "keywords/$seen": true } };
  expect(cache.mutations).toEqual([asked]);
  // The message of this rig stays unread, as one the server refused to mark does.
  fireEvent.click(screen.getByRole("button", { expanded: true }));
  fireEvent.click(screen.getByRole("button", { expanded: false }));
  await screen.findByText(/De meerprijs/);
  expect(cache.mutations).toEqual([asked, asked]);
  cleanup();
  const read = renderCard({ message: planned(newest()) });
  await screen.findByText(/De meerprijs/);
  expect(read.cache.mutations).toEqual([]);
});

test("a cut body offers the whole message, then the download when that comes back cut as well", async () => {
  stubServer();
  const { cache } = renderCard({
    bodies: {
      [NEWEST_ID]: htmlDetail(NEWEST_ID, "<p>the first part</p>", { cut: true }),
      [`${NEWEST_ID}#large`]: htmlDetail(NEWEST_ID, "<p>the first part and more</p>", {
        cut: true,
        large: true,
      }),
    },
  });
  await frame();
  expect(screen.getByText("This message was cut short at 4 MB.")).toBeDefined();
  expect(screen.queryByRole("link", { name: "Download the message" })).toBeNull();
  const asked = vi.spyOn(cache, "body");
  const whole = screen.getByRole("button", { name: "Show the whole message" });
  whole.focus();
  fireEvent.click(whole);
  expect(await screen.findByText("This message was cut short at 12 MB.")).toBeDefined();
  expect(asked).toHaveBeenCalledWith("acc-1", NEWEST_ID, { large: true });
  expect(screen.queryByRole("button", { name: "Show the whole message" })).toBeNull();
  const download = screen.getByRole("link", { name: "Download the message" });
  // The button left with the answer; the body's box holds its focus.
  expect(document.activeElement).toBe(download.closest("[tabindex]"));
  expect(download.getAttribute("href")).toBe(
    "/api/jmap/acc-1/download/u1/e-3/message.eml?type=message%2Frfc822",
  );
  expect(download.hasAttribute("download")).toBe(true);
});

const TOO_LONG = "This message is too long to show in full.";
const DOWNLOAD = "/api/jmap/acc-1/download/u1/e-3/message.eml?type=message%2Frfc822";

test("a plain message past the lines the card draws stops there with the download, cut by the server or not", async () => {
  stubServer();
  const long = `${"line\n".repeat(PLAIN_LINES_MAX)}past the bound`;
  renderCard({ bodies: { [NEWEST_ID]: textDetail(NEWEST_ID, long) } });
  expect(await screen.findByText(TOO_LONG)).toBeDefined();
  expect(screen.queryByText(/past the bound/)).toBeNull();
  expect(screen.getByRole("link", { name: "Download the message" }).getAttribute("href")).toBe(
    DOWNLOAD,
  );
  cleanup();
  // A larger ask shows no more lines, so the notice offers the download at once.
  renderCard({ bodies: { [NEWEST_ID]: textDetail(NEWEST_ID, long, { cut: true }) } });
  expect(await screen.findByText(TOO_LONG)).toBeDefined();
  expect(screen.queryByText(/cut short/)).toBeNull();
  expect(screen.queryByRole("button", { name: "Show the whole message" })).toBeNull();
  expect(screen.getByRole("link", { name: "Download the message" })).toBeDefined();
  cleanup();
  renderCard({ bodies: { [NEWEST_ID]: textDetail(NEWEST_ID, "line\n".repeat(PLAIN_LINES_MAX)) } });
  await screen.findByText(/line/);
  expect(screen.queryByText(TOO_LONG)).toBeNull();
});

test("a plain message shows before the policies list answers; an HTML message waits for it", async () => {
  const held = Promise.withResolvers<Response>();
  vi.stubGlobal("fetch", () => held.promise);
  renderCard({ bodies: { [NEWEST_ID]: textDetail(NEWEST_ID, "plain words") } });
  expect(await screen.findByText("plain words")).toBeDefined();
  cleanup();
  const { cache } = renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, "<p>marked up</p>") },
  });
  const asked = vi.spyOn(cache, "body");
  await waitFor(() => {
    expect(asked).toHaveBeenCalledOnce();
  });
  await act(async () => {
    await asked.mock.results[0]?.value;
  });
  expect(screen.getByRole("status", { name: "Loading…" })).toBeDefined();
  expect(screen.queryByTitle(/^Message from/)).toBeNull();
  held.resolve(Response.json([]));
  await frame();
});

test("offline the whole message is asked at once and a failure says so", async () => {
  stubServer();
  const { cache } = renderCard({
    bodies: {
      [NEWEST_ID]: htmlDetail(NEWEST_ID, "<p>the first part</p>", { cut: true }),
      [`${NEWEST_ID}#large`]: new JmapError("unavailable"),
    },
  });
  await frame();
  const asked = vi.spyOn(cache, "body");
  onlineManager.setOnline(false);
  fireEvent.click(screen.getByRole("button", { name: "Show the whole message" }));
  expect(
    await screen.findByText("Couldn’t load this message. Your mail is safe; nothing was lost."),
  ).toBeDefined();
  expect(asked).toHaveBeenCalledWith("acc-1", NEWEST_ID, { large: true });
  const whole = screen.getByRole("button", { name: "Show the whole message" });
  expect(whole.getAttribute("aria-busy")).not.toBe("true");
});

// The engine of the test reads the relative color syntax and asks for no forced colors.
function darkTheme(): void {
  document.documentElement.dataset["theme"] = "dark";
  vi.stubGlobal("CSS", { supports: () => true });
}

test("in the dark theme a light-only message is adapted and the head offers to show it as sent", async () => {
  stubServer();
  darkTheme();
  renderCard({ bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML) } });
  const shown = await frame();
  expect(shown.srcdoc).toContain("oklch(from");
  expect(shown.hasAttribute("data-canvas")).toBe(false);
  const revert = screen.getByRole("button", { name: "Show original colors" });
  expect(revert.hasAttribute("data-pressed")).toBe(false);
  fireEvent.click(revert);
  const pressed = screen.getByRole("button", { name: "Adapt colors" });
  expect(pressed.hasAttribute("data-pressed")).toBe(true);
  const asSent = await frame();
  expect(asSent.srcdoc).not.toContain("oklch(from");
  expect(asSent.getAttribute("data-canvas")).toBe("as-sent");
  fireEvent.click(pressed);
  expect(screen.getByRole("button", { name: "Show original colors" })).toBeDefined();
});

test("under Show as sent the button starts pressed; a message with dark rules of its own has none", async () => {
  stubServer();
  darkTheme();
  renderCard({
    bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML) },
    preferences: { darkMail: "original" },
  });
  const shown = await frame();
  expect(shown.getAttribute("data-canvas")).toBe("as-sent");
  expect(screen.getByRole("button", { name: "Adapt colors" }).hasAttribute("data-pressed")).toBe(
    true,
  );
  cleanup();
  renderCard({ bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, DECLARES_DARK) } });
  const own = await frame();
  expect(own.hasAttribute("data-canvas")).toBe(false);
  expect(screen.queryByRole("button", { name: /colors/ })).toBeNull();
});

test("in the dark theme an engine that cannot adapt shows the message as sent and offers no revert", async () => {
  stubServer();
  document.documentElement.dataset["theme"] = "dark";
  vi.stubGlobal("CSS", { supports: () => false });
  renderCard({ bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML) } });
  const shown = await frame();
  expect(shown.srcdoc).not.toContain("oklch(from");
  expect(shown.getAttribute("data-canvas")).toBe("as-sent");
  expect(screen.queryByRole("button", { name: /colors/ })).toBeNull();
});

test("in the light theme no revert is offered and a folded card carries no icon", async () => {
  stubServer();
  renderCard({ bodies: { [NEWEST_ID]: htmlDetail(NEWEST_ID, NEWSLETTER_HTML) } });
  await frame();
  expect(screen.queryByRole("button", { name: /colors/ })).toBeNull();
  cleanup();
  renderCard({ expanded: false });
  expect(await screen.findAllByRole("button")).toHaveLength(1);
  expect(screen.queryByTitle(/^Message from/)).toBeNull();
});
