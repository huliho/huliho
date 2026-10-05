// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { TriangleAlert } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import { BrandMark } from "../design-system/brand-mark";
import { Button } from "../design-system/button";
import screenCard from "../design-system/screen-card.module.css";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { planLink, targetPieces } from "./plan";
import type { Plan, Reason } from "./plan";
import styles from "./open-link.module.css";

// How long a tab gets to close before the app's own start loads in it:
// a browser closes only the tabs a page may close.
const CLOSE_WAIT_MS = 100;

// What the page asks of the tab it runs in.
export interface Tab {
  // The fragment the tab opened with.
  hash: string;
  hostname: string;
  // Leaves for an address, which takes this page's place in the history.
  replace(url: string): void;
  // Hands an address to the program that opens it; the page stays.
  hand(url: string): void;
  // Closes the tab; one the browser keeps open goes to the mail instead.
  close(): void;
}

function browserTab(): Tab {
  return {
    hash: window.location.hash,
    hostname: window.location.hostname,
    replace: (url) => {
      window.location.replace(url);
    },
    hand: (url) => {
      window.location.assign(url);
    },
    close: () => {
      window.close();
      window.setTimeout(() => {
        window.location.replace("/");
      }, CLOSE_WAIT_MS);
    },
  };
}

function sentenceOf(reason: Reason, locale: Locale): string {
  switch (reason.kind) {
    case "own":
      return m.link_warning_own({ path: reason.path }, { locale });
    case "mismatch":
      return m.link_warning_mismatch({ text: reason.text, host: reason.host }, { locale });
    case "international":
      return m.link_warning_international({ host: reason.host }, { locale });
    default:
      return m.link_warning_unverified({ host: reason.host }, { locale });
  }
}

interface AskProps {
  locale: Locale;
  tab: Tab;
  plan: Extract<Plan, { kind: "ask" }>;
}

// The link with its reason, the target in full and the two ways on.
// Cancel takes the focus, so a key pressed in haste opens nothing.
function Ask({ locale, tab, plan }: AskProps) {
  const sentenceId = useId();
  const cancelRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancelRef.current?.focus();
  }, []);
  const [lead, named, rest] = targetPieces(plan.url, plan.reason);
  return (
    <section className={screenCard.card} aria-labelledby={sentenceId}>
      <div className={styles.lead}>
        <TriangleAlert className={styles.icon} aria-hidden="true" />
        {/* The sentence carries a host or a text the sender chose. */}
        <h2 id={sentenceId} dir="auto" className={styles.sentence}>
          {sentenceOf(plan.reason, locale)}
        </h2>
      </div>
      <p dir="ltr" className={styles.target}>
        <span>{lead}</span>
        <span className={styles.named}>{named}</span>
        <span>{rest}</span>
      </p>
      <div className={styles.actions}>
        <Button
          onClick={() => {
            tab.replace(plan.url);
          }}
        >
          {m.link_warning_open({}, { locale })}
        </Button>
        <Button
          ref={cancelRef}
          variant="primary"
          aria-describedby={sentenceId}
          onClick={() => {
            tab.close();
          }}
        >
          {m.cancel_action({}, { locale })}
        </Button>
      </div>
    </section>
  );
}

interface NoticeProps {
  locale: Locale;
  tab: Tab;
  plan: Exclude<Plan, { kind: "ask" }>;
}

// A link on its way to a page says where it goes. One that opens
// nothing says so and one handed to the mail program says that; both
// stay, with the way out.
function Notice({ locale, tab, plan }: NoticeProps) {
  if (plan.kind === "leave") {
    return (
      <p dir="auto" className={styles.opening}>
        {m.link_opening({ host: plan.host }, { locale })}
      </p>
    );
  }
  const invalid = plan.kind === "invalid";
  return (
    <section className={screenCard.card}>
      <div className={styles.lead}>
        {invalid ? <TriangleAlert className={styles.icon} aria-hidden="true" /> : null}
        <h2 className={styles.sentence}>
          {invalid ? m.link_warning_invalid({}, { locale }) : m.link_opening_mail({}, { locale })}
        </h2>
      </div>
      <div className={styles.actions}>
        <Button
          variant="primary"
          onClick={() => {
            tab.close();
          }}
        >
          {m.link_close({}, { locale })}
        </Button>
      </div>
    </section>
  );
}

// The page for one tab. A mail address goes to the mail program and the
// tab stays for the reader to close: a mail handler on the web takes
// the tab itself and a browser may ask in it which program to use.
export function OpenLinkPage({ tab }: { tab: Tab }) {
  const locale = useLocale();
  const [plan] = useState(() => planLink(tab.hash, tab.hostname));
  useEffect(() => {
    if (plan.kind === "leave") {
      tab.replace(plan.url);
    }
    if (plan.kind === "mail") {
      tab.hand(plan.url);
    }
  }, [plan, tab]);
  return (
    <main className={styles.screen}>
      <div className={styles.column}>
        <BrandMark heading />
        {plan.kind === "ask" ? (
          <Ask locale={locale} tab={tab} plan={plan} />
        ) : (
          <Notice locale={locale} tab={tab} plan={plan} />
        )}
      </div>
    </main>
  );
}

// The page a link of a mail opens, in the tab that link opened.
export function OpenLink() {
  const [tab] = useState(browserTab);
  return <OpenLinkPage tab={tab} />;
}
