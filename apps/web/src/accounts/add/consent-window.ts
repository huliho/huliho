// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The provider's consent page is laid out for a phone-sized window.
const CONSENT_WINDOW_WIDTH = 520;
const CONSENT_WINDOW_HEIGHT = 720;
const CONSENT_WINDOW_FEATURES = `popup,width=${String(CONSENT_WINDOW_WIDTH)},height=${String(CONSENT_WINDOW_HEIGHT)}`;

// Opens the consent window from a click, so no popup blocker gets a say,
// and cuts its opener while the page is still ours: the provider must
// never reach this app. Null when the browser kept the window closed.
export function openConsentWindow(url: string | null): WindowProxy | null {
  const handle = window.open("", "_blank", CONSENT_WINDOW_FEATURES);
  if (handle === null) {
    return null;
  }
  handle.opener = null;
  if (url !== null) {
    handle.location.assign(url);
  }
  return handle;
}
