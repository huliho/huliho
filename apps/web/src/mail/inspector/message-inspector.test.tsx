// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { commandsSnapshot } from "../../commands/registry";
import { PASSED } from "../body-fixtures";
import { MessageInspector } from "./message-inspector";
import type { InspectedBody, MessageInspectorProps } from "./message-inspector";

const DOWNLOAD = "/api/jmap/acc-1/download/u1/e-3/message.eml?type=message%2Frfc822";
const SOURCE = "From: a@b.example\r\nSubject: hi\r\n\r\nhello\r\n";
const PARTIAL_CONTENT = 206;

function body(over: Partial<InspectedBody> = {}): InspectedBody {
  return {
    authentication: PASSED,
    plain: { text: <p>the plain words</p>, short: null },
    download: DOWNLOAD,
    source: { key: ["acc-1", "source", "e-3"], url: DOWNLOAD },
    ...over,
  };
}

interface Rendered {
  onClose: ReturnType<typeof vi.fn<() => void>>;
  opener: HTMLButtonElement;
}

// The inspector open over a button that opened it, with a route that
// answers the source as told.
function renderInspector(props: Partial<MessageInspectorProps> = {}): Rendered {
  const onClose = vi.fn<() => void>();
  const opener = document.createElement("button");
  document.body.append(opener);
  opener.focus();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MessageInspector
        locale="en"
        open
        onClose={onClose}
        onClosed={() => undefined}
        opener={opener}
        rendered={<p>the rendered words</p>}
        body={body()}
        {...props}
      />
    </QueryClientProvider>,
  );
  return { onClose, opener };
}

// The route behind the source tab, answering as told; the mock counts its calls.
function stubSource(status: number, text: string, headers: Record<string, string> = {}) {
  const fetching = vi.fn<() => Promise<Response>>(() =>
    Promise.resolve(new Response(text, { status, headers })),
  );
  vi.stubGlobal("fetch", fetching);
  return fetching;
}

// The receiving server's sentence, read whole from its paragraph.
function lineOf(dialog: HTMLElement): string | null {
  return (
    within(dialog)
      .queryByText(/Checked by/)
      ?.closest("p")?.textContent ?? null
  );
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  for (const stray of document.querySelectorAll("body > button")) {
    stray.remove();
  }
});

test("the dialog is named, carries the line above its tabs, shows the rendered message first and puts the first focus on its close button", async () => {
  stubSource(200, SOURCE);
  renderInspector();
  const dialog = await screen.findByRole("dialog", { name: "Message details" });
  expect(lineOf(dialog)).toBe(
    "Checked by mx.fastmail.example: SPF passed, DKIM passed, DMARC passed.",
  );
  const tabs = within(dialog).getAllByRole("tab");
  expect(tabs.map((tab) => tab.textContent)).toEqual(["Rendered", "Plain text", "Source"]);
  expect(within(dialog).getByRole("tabpanel").textContent).toBe("the rendered words");
  await waitFor(() => {
    expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Close" }));
  });
  const download = within(dialog).getByRole("link", { name: "Download the message" });
  expect(download.getAttribute("href")).toBe(DOWNLOAD);
  expect(download.hasAttribute("download")).toBe(true);
});

test("the plain tab shows the text with the sentence of why it stops short and no button; a message without text gets its own sentence", async () => {
  stubSource(200, SOURCE);
  renderInspector();
  fireEvent.click(await screen.findByRole("tab", { name: "Plain text" }));
  expect(screen.getByRole("tabpanel").textContent).toBe("the plain words");
  cleanup();
  renderInspector({
    body: body({ plain: { text: <p>the first lines</p>, short: { kind: "lines" } } }),
    initialTab: "plain",
  });
  expect((await screen.findByRole("tabpanel")).textContent).toBe(
    "the first linesThis message is too long to show in full.",
  );
  cleanup();
  renderInspector({
    body: body({ plain: { text: <p>the first part</p>, short: { kind: "size", size: "4 MB" } } }),
    initialTab: "plain",
  });
  expect((await screen.findByRole("tabpanel")).textContent).toBe(
    "the first partThis message was cut short at 4 MB.",
  );
  // The way to the rest is the download in the foot alone.
  expect(screen.queryByRole("button", { name: "Show the whole message" })).toBeNull();
  expect(screen.getAllByRole("link", { name: "Download the message" })).toHaveLength(1);
  cleanup();
  renderInspector({ body: body({ plain: null }) });
  fireEvent.click(await screen.findByRole("tab", { name: "Plain text" }));
  expect(screen.getByRole("tabpanel").textContent).toBe("No plain-text version of this message.");
});

test("the source tab reads the message from the route once it is shown and marks a cut one", async () => {
  const fetching = stubSource(200, SOURCE);
  renderInspector();
  await screen.findByRole("dialog");
  expect(fetching).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("tab", { name: "Source" }));
  const source = await screen.findByText(/Subject: hi/);
  expect(source.tagName).toBe("PRE");
  expect(source.getAttribute("dir")).toBe("ltr");
  expect(fetching).toHaveBeenCalledOnce();
  expect(screen.queryByText(/Showing the first/)).toBeNull();
  cleanup();
  stubSource(PARTIAL_CONTENT, SOURCE, { "content-range": "bytes 0-39/4000000" });
  renderInspector({ initialTab: "source" });
  expect(await screen.findByText("Showing the first 512 kB of this message.")).toBeDefined();
});

test("a source that cannot be read says so with Try again; the still lines stand while the body is on its way", async () => {
  stubSource(502, "");
  renderInspector({ initialTab: "source" });
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Couldn’t load this message.");
  stubSource(200, SOURCE);
  fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));
  expect(await screen.findByText(/Subject: hi/)).toBeDefined();
  cleanup();
  renderInspector({ body: null, initialTab: "source" });
  await screen.findByRole("dialog");
  expect(screen.getByRole("status", { name: "Loading…" })).toBeDefined();
  expect(screen.queryByRole("link", { name: "Download the message" })).toBeNull();
  expect(lineOf(screen.getByRole("dialog"))).toBeNull();
});

test("offline a source this device never got says so without Try again; back online it lands by itself", async () => {
  const onLine = vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  const fetching = vi.fn<() => Promise<Response>>(() =>
    Promise.reject(new TypeError("Failed to fetch")),
  );
  vi.stubGlobal("fetch", fetching);
  renderInspector({ initialTab: "source" });
  // The query client hears the network from the window once it is mounted.
  window.dispatchEvent(new Event("offline"));
  expect(
    (await screen.findByText("Offline: this message isn’t stored on this device.")).getAttribute(
      "role",
    ),
  ).toBe("status");
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  expect(fetching).toHaveBeenCalledOnce();
  stubSource(200, SOURCE);
  onLine.mockReturnValue(true);
  window.dispatchEvent(new Event("online"));
  expect(await screen.findByText(/Subject: hi/)).toBeDefined();
});

test("Escape and the close button close it; the registry's Escape closes it too while it stands", async () => {
  stubSource(200, SOURCE);
  const { onClose } = renderInspector();
  const dialog = await screen.findByRole("dialog");
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(2);
  const closing = commandsSnapshot().findLast((one) => one.id === "inspector.close");
  expect(closing?.keys).toEqual([{ key: "Escape" }]);
  closing?.run();
  expect(onClose).toHaveBeenCalledTimes(3);
});

test("the focus goes back to the opener once the dialog is gone", async () => {
  stubSource(200, SOURCE);
  const { opener } = renderInspector();
  await screen.findByRole("dialog");
  cleanup();
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  expect(document.body.contains(opener)).toBe(true);
});
