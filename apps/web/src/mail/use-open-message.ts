// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BodyDetail, DarkMail, EmailHeader, MailCache } from "@huliho/core";
import { preferencesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { RefObject } from "react";

import { useLinkKey } from "../open/link-key";
import type { Locale } from "../paraglide/runtime.js";
import { useOnline } from "../shell/use-online";
import { useTheme } from "../theme/use-theme";
import type { Theme } from "../theme/use-theme";
import { canAdapt } from "./body/colors";
import { buildFrameDocument } from "./body/frame-document";
import type { FrameDocument } from "./body/frame-document";
import { useFrameStyle } from "./body/use-frame-style";
import { shapeOf, textOf } from "./body-shape";
import { cutOf, frameTitle, messageDownload } from "./body-view";
import type { BodyView, Cut } from "./message-body";
import { firstLines } from "./plain-text";
import type { LinkPolicy } from "./plain-text";
import type { RemoteContentBarProps } from "./remote-content-bar";
import { useBodyQuery } from "./use-body-query";
import type { BodyAsk } from "./use-body-query";
import { useRemoteContent } from "./use-remote-content";
import type { RemoteContent } from "./use-remote-content";

// What a reader who never chose gets: a light-only message adapted to
// the dark theme.
const DEFAULT_DARK_MAIL: DarkMail = "adapt";

// The control that shows a light-only message as sent and adapts it
// again, for this message alone.
export interface Revert {
  original: boolean;
  toggle: () => void;
}

export interface OpenMessage {
  view: BodyView;
  // The bar above the body, null when the message names no remote image.
  bar: RemoteContentBarProps | null;
  // Null where the adaptation would change nothing.
  revert: Revert | null;
}

interface Facts {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  email: EmailHeader;
}

interface Pieces {
  locale: Locale;
  email: EmailHeader;
  theme: Theme;
  ask: BodyAsk;
  online: boolean;
  // The frame's document, null until everything it takes is known.
  built: FrameDocument | null;
  links: LinkPolicy;
}

// A message shown as sent in the dark theme stands on its own white page.
function asSent(theme: Theme, built: FrameDocument): boolean {
  return theme === "dark" && !built.adapted && !built.declaresDark;
}

// The view of a text: its first lines, with the download for a longer
// one, since a larger ask shows no more of it; else the cut as asked.
function textViewOf(
  detail: BodyDetail,
  email: EmailHeader,
  links: LinkPolicy,
  cut: Cut | null,
): BodyView {
  const lines = firstLines(textOf(detail.body));
  return {
    kind: "text",
    text: lines.text,
    flowed: detail.body.flowed,
    links,
    cut: lines.more ? { kind: "lines", download: messageDownload(detail, email) } : cut,
  };
}

function viewOf({ locale, email, theme, ask, online, built, links }: Pieces): BodyView {
  const { query } = ask;
  if (query.isError) {
    return online ? { kind: "error", retry: () => void query.refetch() } : { kind: "offline" };
  }
  if (query.data === null) {
    return { kind: "gone" };
  }
  const detail = query.data;
  if (detail === undefined) {
    return { kind: "loading" };
  }
  const shape = shapeOf(detail.body);
  const cut = cutOf(detail, email, locale, { show: ask.showWhole, pending: ask.wholePending });
  if (shape === "html") {
    return built === null
      ? { kind: "loading" }
      : {
          kind: "html",
          html: built.html,
          title: frameTitle(email, locale),
          asSent: asSent(theme, built),
          cut,
        };
  }
  if (shape === "text") {
    return textViewOf(detail, email, links, cut);
  }
  return { kind: shape };
}

function barOf(locale: Locale, remote: RemoteContent): RemoteContentBarProps {
  return {
    locale,
    state: remote.bar,
    sender: remote.sender,
    canAlways: remote.canAlways,
    pending: remote.pending,
    onLoadOnce: remote.loadOnce,
    onAllow: remote.allow,
    onStop: remote.stop,
  };
}

interface Build {
  detail: BodyDetail | null;
  theme: Theme;
  adapt: boolean;
  remote: RemoteContent;
  links: LinkPolicy;
  style: ReturnType<typeof useFrameStyle>;
}

// The frame's document for an HTML message, once everything it takes
// is known; rebuilt for every view and never stored.
function buildOf({ detail, theme, adapt, remote, links, style }: Build): FrameDocument | null {
  if (detail === null || style === null || !remote.ready || shapeOf(detail.body) !== "html") {
    return null;
  }
  return buildFrameDocument(detail.body, {
    theme,
    adapt,
    remote: remote.remote,
    ownHost: links.ownHost,
    download: detail.download,
    linkKey: links.linkKey,
    style,
  });
}

interface Adaptation {
  // Whether a light-only message is adapted in this view.
  adapt: boolean;
  // The revert control for a built document, null where the adaptation
  // would change nothing.
  revertOf: (built: FrameDocument | null) => Revert | null;
}

// The reader's say over the dark adaptation of this message: the
// setting, until the revert control turns it around for this view.
function useAdaptation(theme: Theme): Adaptation {
  const darkMail = useQuery(preferencesQueryOptions).data?.darkMail ?? DEFAULT_DARK_MAIL;
  const [original, setOriginal] = useState<boolean | null>(null);
  const wantsOriginal = original ?? darkMail === "original";
  const adaptable = theme === "dark" && canAdapt();
  return {
    adapt: adaptable && !wantsOriginal,
    revertOf: (built) =>
      built === null || !adaptable || built.declaresDark
        ? null
        : {
            original: wantsOriginal,
            toggle: () => {
              setOriginal(!wantsOriginal);
            },
          },
  };
}

// Everything an open card shows beside its head: the body in its state,
// the bar when the message names remote images and the revert control
// when the dark adaptation has something to undo. `slotRef` names the
// box the body stands in, whose type and colors the frame takes.
export function useOpenMessage(slotRef: RefObject<HTMLElement | null>, facts: Facts): OpenMessage {
  const { locale, cache, accountId, email } = facts;
  const theme = useTheme();
  const style = useFrameStyle(slotRef);
  const online = useOnline();
  const adaptation = useAdaptation(theme);
  const ask = useBodyQuery(cache, accountId, email.id, locale);
  const detail = ask.query.data ?? null;
  const remote = useRemoteContent(locale, email, detail?.body.authentication ?? null);
  // Without the device's key a link carries none and the open route asks first.
  const links: LinkPolicy = { ownHost: window.location.hostname, linkKey: useLinkKey() ?? "" };
  const built = buildOf({ detail, theme, adapt: adaptation.adapt, remote, links, style });
  return {
    view: viewOf({ locale, email, theme, ask, online, built, links }),
    bar: built !== null && built.remote > 0 ? barOf(locale, remote) : null,
    revert: adaptation.revertOf(built),
  };
}
