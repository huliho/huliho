// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { classifyLinkUrl } from "@huliho/core";

import { openHref } from "../../open/fragment";

export interface LinkPolicy {
  ownHost: string;
  // The device's key, which the open route asks of a link.
  linkKey: string;
}

// Where a link of the mail leads once it passed the policy: the app's
// own route, with the target in the fragment. Null for a link the
// policy drops and for one too long to carry.
function opened(anchor: HTMLAnchorElement, policy: LinkPolicy): [string, string] | null {
  const href = anchor.getAttribute("href");
  const target = href === null ? null : classifyLinkUrl(href, policy.ownHost);
  if (target === null || target.kind === "dropped") {
    return null;
  }
  const address = openHref({ target: target.url, text: anchor.textContent, key: policy.linkKey });
  return address === null ? null : [address, target.url];
}

// Points every link of a sanitized body at the open route. The title
// shows where the link leads, which its address hides; a link that
// opens nothing stays as its text.
export function rewriteLinks(body: HTMLElement, policy: LinkPolicy): void {
  for (const anchor of body.querySelectorAll("a")) {
    const link = opened(anchor, policy);
    if (link === null) {
      for (const name of ["href", "target", "rel"]) {
        anchor.removeAttribute(name);
      }
    } else {
      const [address, target] = link;
      anchor.setAttribute("href", address);
      anchor.setAttribute("title", target);
    }
  }
}
