// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Runs before each page's audits. The mail route gets the session cookie
// the preview's API reads (e2e/preview-api.ts) and the sign-in screen goes
// without; the session's answer is read back, so a page that would load
// in the wrong state fails the run instead.
const SESSION_COOKIE = { name: "huliho_session", value: "preview" };
const SIGN_IN_PATH = "/sign-in";
const SIGNED_IN = 200;
const SIGNED_OUT = 401;

/**
 * @param {import("puppeteer-core").Browser} browser
 * @param {{ url: string }} context
 */
module.exports = async (browser, { url }) => {
  const { hostname, origin, pathname } = new URL(url);
  const signedIn = pathname !== SIGN_IN_PATH;
  if (signedIn) {
    await browser.setCookie({ ...SESSION_COOKIE, domain: hostname, path: "/" });
  } else {
    await browser.deleteMatchingCookies({ name: SESSION_COOKIE.name, url: origin });
  }
  const page = await browser.newPage();
  const status = (await page.goto(`${origin}/api/session`))?.status();
  await page.close();
  if (status !== (signedIn ? SIGNED_IN : SIGNED_OUT)) {
    throw new Error(`the session answered ${String(status)} ahead of ${pathname}`);
  }
};
