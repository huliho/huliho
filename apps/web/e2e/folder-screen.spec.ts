// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The account's folder screen: the six roles, the hidden folders and the
// mailboxes the server does not open, against a server that lets the
// user move them.

import type { Locator, Page, Route } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { FIXED_NOW } from "./account-card";
import { accountRow, mockAccounts } from "./account-mocks";
import type { MailServer } from "./mail-answers";
import { FOLDER_MAILBOXES, mockMail } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import { settled } from "./thread-pane";

const ACCOUNT = "Fastmail";
const SCREEN_PATH = "/settings/accounts/acc-1/folders";
const ACCOUNTS_PATH = "/settings/accounts";

function hidden(id: string, name: string, parentId: string | null = null): MailboxBody {
  return { ...row("mb-facturen"), id, name, parentId, isSubscribed: false, totalEmails: 0 };
}

function row(id: string): MailboxBody {
  const found = FOLDER_MAILBOXES.find((mailbox) => mailbox.id === id);
  if (found === undefined) {
    throw new Error(`no mailbox ${id}`);
  }
  return found;
}

function closed(id: string, name: string, isSubscribed: boolean): MailboxBody {
  return {
    ...hidden(id, name),
    isSubscribed,
    myRights: {
      ...row("mb-facturen").myRights,
      mayReadItems: false,
      mayAddItems: false,
      mayRemoveItems: false,
      maySetSeen: false,
      maySetKeywords: false,
      mayRename: false,
      mayDelete: false,
    },
  };
}

const WITH_CHOICES: MailboxBody[] = [
  ...FOLDER_MAILBOXES,
  hidden("mb-kelder", "Kelder"),
  hidden("mb-oud", "Oud", "mb-kelder"),
  closed("mb-gedeeld", "Gedeeld", true),
];

// Every role mailbox answers no rename, as a provider that keeps them.
const FIXED: MailboxBody[] = FOLDER_MAILBOXES.map((mailbox) =>
  mailbox.role === null
    ? mailbox
    : { ...mailbox, myRights: { ...mailbox.myRights, mayRename: false } },
);

function unsubscribed(mailbox: MailboxBody): MailboxBody {
  return { ...mailbox, isSubscribed: false };
}

// Nothing subscribed, as the compose Cyrus has it.
const NOTHING_SUBSCRIBED: MailboxBody[] = [
  ...FOLDER_MAILBOXES,
  closed("mb-gedeeld", "Gedeeld", false),
].map(unsubscribed);

async function mocked(page: Page, mailboxes: MailboxBody[]): Promise<MailServer> {
  await mockSignedIn(page);
  const { server } = await mockMail(page, mailboxes);
  await mockAccounts(page, [accountRow(FIXED_NOW)]);
  await mockPreferences(page, { readingPane: "right" });
  await page.clock.setFixedTime(FIXED_NOW);
  return server;
}

async function openScreen(
  page: Page,
  mailboxes: MailboxBody[] = WITH_CHOICES,
): Promise<MailServer> {
  const server = await mocked(page, mailboxes);
  await page.goto(SCREEN_PATH);
  await expect(page.getByRole("combobox", { name: "Inbox" })).toBeVisible();
  return server;
}

function roleSelect(page: Page, name: string): Locator {
  return page.getByRole("combobox", { name, exact: true });
}

function toast(page: Page, text: string): Locator {
  return page.getByRole("dialog").filter({ hasText: text });
}

function roleOf(server: MailServer, id: string): unknown {
  return server.mailboxes.find((mailbox) => mailbox["id"] === id)?.["role"];
}

function subscribed(server: MailServer, id: string): unknown {
  return server.mailboxes.find((mailbox) => mailbox["id"] === id)?.["isSubscribed"];
}

test("the Folders link on the account's row opens the screen under the account's name", async ({
  page,
}) => {
  await mocked(page, WITH_CHOICES);
  await page.goto(ACCOUNTS_PATH);
  await page.getByRole("link", { name: `Folders of ${ACCOUNT}` }).click();
  await expect(page).toHaveURL(SCREEN_PATH);
  await expect(page.getByRole("heading", { level: 2, name: ACCOUNT })).toBeVisible();
  await expect(page.getByRole("heading", { level: 2, name: "Hidden folders" })).toBeVisible();
  await expect(page.getByRole("combobox")).toHaveCount(6);
  await expect(roleSelect(page, "Archive")).toHaveValue("mb-archive");
  await expect(roleSelect(page, "Inbox")).toBeDisabled();
  await expect(roleSelect(page, "Archive")).toBeEnabled();
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("a visit by its address starts the worker; an account the session lacks goes to the list", async ({
  page,
}) => {
  await openScreen(page);
  await expect(roleSelect(page, "Sent")).toHaveValue("mb-sent");
  await page.goto("/settings/accounts/acc-gone/folders");
  await expect(page).toHaveURL(ACCOUNTS_PATH);
});

test("a role moved sends the holder's null and the taker's role in one call; the tree follows", async ({
  page,
}) => {
  const server = await openScreen(page);
  const bodies: string[] = [];
  await page.context().route("**/api/jmap/*", async (route: Route) => {
    bodies.push(route.request().postData() ?? "");
    await route.fallback();
  });
  await roleSelect(page, "Archive").selectOption("mb-facturen");
  await expect(roleSelect(page, "Archive")).toHaveValue("mb-facturen");
  await expect.poll(() => roleOf(server, "mb-facturen")).toBe("archive");
  expect(roleOf(server, "mb-archive")).toBeNull();
  const written = bodies.filter((body) => body.includes("Mailbox/set"));
  expect(written).toHaveLength(1);
  expect(written[0]).toContain(
    '"update":{"mb-archive":{"role":null},"mb-facturen":{"role":"archive"}}',
  );
  await page.goto("/mail/acc-1/mb-inbox");
  const tree = page.getByRole("tree");
  await expect(tree.getByRole("treeitem", { name: /^Facturen/ })).toBeVisible();
  const names = await tree.getByRole("treeitem").allTextContents();
  expect(names.findIndex((name) => name.startsWith("Facturen"))).toBeLessThan(
    names.findIndex((name) => name.startsWith("Trash")),
  );
});

test("a mailbox that holds another role takes this one and the other row falls to None", async ({
  page,
}) => {
  const server = await openScreen(page);
  await roleSelect(page, "Archive").selectOption("mb-trash");
  await expect.poll(() => roleOf(server, "mb-trash")).toBe("archive");
  await expect(roleSelect(page, "Trash")).toHaveValue("");
  await expect(roleSelect(page, "Archive")).toHaveValue("mb-trash");
});

test("None takes the role away and the mailbox stays a folder", async ({ page }) => {
  const server = await openScreen(page);
  await roleSelect(page, "Sent").selectOption("");
  await expect.poll(() => roleOf(server, "mb-sent")).toBeNull();
  await expect(roleSelect(page, "Sent")).toHaveValue("");
});

test("a refusal says so in a toast and the row goes back to what the server holds", async ({
  page,
}) => {
  const server = await openScreen(page);
  server.folderRefusal = "forbidden";
  await roleSelect(page, "Archive").selectOption("mb-facturen");
  await expect(toast(page, "The server refused. Try again.")).toBeVisible();
  await expect(roleSelect(page, "Archive")).toHaveValue("mb-archive");
  expect(roleOf(server, "mb-archive")).toBe("archive");
});

test("a server that keeps its role mailboxes takes no change and says so", async ({ page }) => {
  await openScreen(page, FIXED);
  for (const name of ["Inbox", "Drafts", "Sent", "Archive", "Junk", "Trash"]) {
    await expect(roleSelect(page, name)).toBeDisabled();
  }
  await expect(page.getByText("The server sets the folders you can’t change here.")).toBeVisible();
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("Show brings a hidden folder back alone, its subfolder stays listed and the focus stays", async ({
  page,
}) => {
  const server = await openScreen(page);
  await expect(
    page.getByRole("list", { name: "Hidden folders" }).getByRole("listitem"),
  ).toHaveCount(2);
  const show = page.getByRole("button", { name: "Show Kelder", exact: true });
  await show.focus();
  await page.keyboard.press("Enter");
  await expect(toast(page, "Folder shown.")).toBeVisible();
  await expect(show).toHaveCount(0);
  expect(subscribed(server, "mb-kelder")).toBe(true);
  expect(subscribed(server, "mb-oud")).toBe(false);
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(false);
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Show Kelder / Oud" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByText("No hidden folders")).toBeVisible();
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(false);
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("a mailbox the server does not open is hidden and comes back, the hidden list following", async ({
  page,
}) => {
  const server = await openScreen(page);
  const choice = page.getByRole("radiogroup", { name: "Gedeeld" });
  await expect(choice.getByRole("radio", { name: "Show as a link" })).toBeChecked();
  await choice.getByRole("radio", { name: "Hide" }).click();
  await expect.poll(() => subscribed(server, "mb-gedeeld")).toBe(false);
  await expect(choice.getByRole("radio", { name: "Hide" })).toBeChecked();
  await expect(page.getByRole("button", { name: "Show Gedeeld" })).toBeVisible();
  await choice.getByRole("radio", { name: "Show as a link" }).click();
  await expect.poll(() => subscribed(server, "mb-gedeeld")).toBe(true);
  await expect(page.getByRole("button", { name: "Show Gedeeld" })).toHaveCount(0);
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("an account that subscribes nothing hides nothing and offers no choice", async ({ page }) => {
  await openScreen(page, NOTHING_SUBSCRIBED);
  await expect(page.getByText("No hidden folders")).toBeVisible();
  await expect(page.getByRole("radiogroup")).toHaveCount(0);
});

test("offline every control holds and one sentence says why", async ({ page }) => {
  const server = await openScreen(page);
  await page.context().setOffline(true);
  await expect(page.getByText("Changes need a connection.")).toBeVisible();
  await expect(roleSelect(page, "Archive")).toBeDisabled();
  const show = page.getByRole("button", { name: "Show Kelder", exact: true });
  await show.click({ force: true });
  await expect(page.getByRole("radio", { name: "Hide" })).toBeDisabled();
  expect(subscribed(server, "mb-kelder")).toBe(false);
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  await page.context().setOffline(false);
  await expect(page.getByText("Changes need a connection.")).toHaveCount(0);
  await expect(roleSelect(page, "Archive")).toBeEnabled();
});

test("a tree that does not load offers Try again", async ({ page }) => {
  await mocked(page, WITH_CHOICES);
  let failing = true;
  await page
    .context()
    .route("**/api/jmap/*", (route: Route) =>
      failing ? route.fulfill({ status: 500, body: "" }) : route.fallback(),
    );
  await page.goto(SCREEN_PATH);
  const retry = page.getByRole("button", { name: "Try again" });
  await expect(page.getByRole("alert")).toContainText("Couldn’t load the folders.");
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  failing = false;
  await retry.click();
  await expect(page.getByRole("combobox", { name: "Inbox" })).toBeVisible();
});

test("an account that holds no mailbox yet says so and offers nothing", async ({ page }) => {
  await mocked(page, []);
  await page.goto(SCREEN_PATH);
  await expect(page.getByText("This account has no mailboxes to show yet.")).toBeVisible();
  await expect(page.getByRole("combobox")).toHaveCount(0);
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("the screen reads right to left in the pseudo-locale", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await mocked(page, WITH_CHOICES);
  await page.goto(SCREEN_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await expect(page.getByRole("combobox")).toHaveCount(6);
  await settled(page);
  await expect
    .soft(page)
    .toHaveScreenshot("folder-screen-en-XA-light-desktop.png", { fullPage: true });
});

for (const viewport of VIEWPORTS) {
  test.describe(`at ${viewport.name} width`, () => {
    test.use({ viewport });

    for (const theme of THEMES) {
      test(`the screen stands in ${theme}`, async ({ page }) => {
        await page.emulateMedia({ colorScheme: theme });
        await openScreen(page);
        expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
        await settled(page);
        await expect
          .soft(page)
          .toHaveScreenshot(`folder-screen-${theme}-${viewport.name}.png`, { fullPage: true });
      });
    }
  });
}

test.describe("on a phone", () => {
  test.use({ viewport: VIEWPORTS[0] });

  test("a long folder name stays inside the select and the page never scrolls sideways", async ({
    page,
  }) => {
    const long = "Offertes van onderaannemers voor de verbouwing van de zolder en de kelder";
    await openScreen(page, [...WITH_CHOICES, { ...row("mb-facturen"), id: "mb-lang", name: long }]);
    const widths = await page.evaluate(() => [
      document.documentElement.scrollWidth,
      window.innerWidth,
    ]);
    expect(widths[0]).toBeLessThanOrEqual(widths[1] ?? 0);
    const select = await roleSelect(page, "Archive").boundingBox();
    const card = await page.locator("section").first().boundingBox();
    expect((select?.x ?? 0) + (select?.width ?? 0)).toBeLessThanOrEqual(
      (card?.x ?? 0) + (card?.width ?? 0),
    );
  });

  test("the header names the screen and its arrow returns to the accounts", async ({ page }) => {
    await openScreen(page);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Folders");
    await page.getByRole("link", { name: "Back" }).click();
    await expect(page).toHaveURL(ACCOUNTS_PATH);
  });
});
