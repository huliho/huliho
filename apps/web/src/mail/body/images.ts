// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { classifyImageUrl, downloadUrl, remoteImageUrl } from "@huliho/core";
import type { DownloadSource, EmailBody, EmailBodyPart } from "@huliho/core";

const IMAGE_TYPE = "image/";

// The name a part without one is asked under; the frame shows no name.
const UNNAMED = "image";

// How many blocked images of one message get a drawn box. An engine
// draws each box as a document of its own, about a millisecond apiece
// on a mid-range phone, so a mail of thousands of images could hold
// the tab for minutes; past this a blocked image keeps its place blank.
export const BLOCKED_BOXES_MAX = 100;

// What stands in the place of an image URL from a mail: the URL the
// frame loads instead, the sender's own data image, a remote image the
// reader has not allowed or nothing.
export type Placed =
  { kind: "loads"; url: string } | { kind: "data" } | { kind: "blocked" } | { kind: "dropped" };

const DROPPED: Placed = { kind: "dropped" };

export interface ImagePolicy {
  ownHost: string;
  // Whether the reader allowed this sender's remote images.
  remote: boolean;
  download: DownloadSource;
}

// The parts of the message by Content-ID, the first of each.
function partsByCid(body: EmailBody): Map<string, EmailBodyPart> {
  const parts = new Map<string, EmailBodyPart>();
  for (const part of [...body.htmlBody, ...body.attachments]) {
    if (part.cid !== null && !parts.has(part.cid)) {
      parts.set(part.cid, part);
    }
  }
  return parts;
}

// The URL policy over the images of one message: a part of the message
// itself loads from the download route, a remote image through the
// server's proxy once the reader allowed the sender, a data image as it
// is; everything else loads nothing. It keeps the remote URLs it met,
// the parts it showed inline and counts the boxes it gave to blocked images.
export class Images {
  private readonly policy: ImagePolicy;
  private readonly parts: Map<string, EmailBodyPart>;
  private readonly found = new Set<string>();
  private readonly inline = new Set<string>();
  private boxes = 0;

  constructor(body: EmailBody, policy: ImagePolicy) {
    this.policy = policy;
    this.parts = partsByCid(body);
  }

  // How many remote images the message names, allowed or not: each
  // address once, however often an engine lists it.
  get remote(): number {
    return this.found.size;
  }

  // The ids of the parts the message shows inline: one a `cid:` resolved
  // to and an image of the body list. The attachment strip leaves them out.
  get inlineParts(): ReadonlySet<string> {
    return this.inline;
  }

  // Whether one more blocked image of the message gets a drawn box.
  takeBox(): boolean {
    this.boxes += 1;
    return this.boxes <= BLOCKED_BOXES_MAX;
  }

  place(value: string): Placed {
    const source = classifyImageUrl(value, this.policy.ownHost);
    switch (source.kind) {
      case "cid":
        return this.part(this.parts.get(source.cid));
      case "remote":
        this.found.add(source.url);
        return this.policy.remote
          ? { kind: "loads", url: remoteImageUrl(source.url) }
          : { kind: "blocked" };
      case "data":
        return { kind: "data" };
      default:
        return DROPPED;
    }
  }

  // Where an image part of the message loads from; one that loads counts
  // as shown inline. A part of another type loads nothing and neither
  // does one without a blob.
  part(part: EmailBodyPart | undefined): Placed {
    if (part === undefined || part.blobId === null) {
      return DROPPED;
    }
    if (!part.type.toLowerCase().startsWith(IMAGE_TYPE)) {
      return DROPPED;
    }
    if (part.partId !== null) {
      this.inline.add(part.partId);
    }
    const name = part.name === null || part.name === "" ? UNNAMED : part.name;
    const url = downloadUrl(this.policy.download, { blobId: part.blobId, name, type: part.type });
    return { kind: "loads", url };
  }
}
