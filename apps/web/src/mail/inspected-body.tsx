// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BodyDetail, EmailBody, EmailHeader } from "@huliho/core";
import { queryKeys } from "@huliho/state";

import { hasPlainText, textOf } from "./body-shape";
import { messageDownload } from "./body-view";
import type { InspectedBody, InspectedPlain } from "./inspector/message-inspector";
import type { Cut } from "./message-body";
import { PlainText, firstLines } from "./plain-text";
import type { LinkPolicy } from "./plain-text";

// The text as the card draws it, stopping where the card stops: past
// the lines it draws, else at the server's cut. Null for a message
// without a text part.
function plainOf(body: EmailBody, links: LinkPolicy, cut: Cut | null): InspectedPlain | null {
  if (!hasPlainText(body)) {
    return null;
  }
  const lines = firstLines(textOf(body));
  return {
    text: <PlainText text={lines.text} flowed={body.flowed} links={links} />,
    short: lines.more ? { kind: "lines" } : cut,
  };
}

// What the details read of an open card: the body as answered, the
// message's header and account, the link policy of its text and the
// server's cut of the body.
interface Inspection {
  detail: BodyDetail | null;
  email: EmailHeader;
  accountId: string;
  links: LinkPolicy;
  cut: Cut | null;
}

// What the inspector shows of a message once its body is in: the
// receiving server's verdict, the plain text as the card draws it, the
// download and where the source is read from. Null until then.
export function inspectedBodyOf({
  detail,
  email,
  accountId,
  links,
  cut,
}: Inspection): InspectedBody | null {
  if (detail === null) {
    return null;
  }
  const { body } = detail;
  const download = messageDownload(detail, email);
  return {
    authentication: body.authentication,
    plain: plainOf(body, links, cut),
    download,
    source: { key: queryKeys.source(accountId, email.id), url: download },
  };
}
