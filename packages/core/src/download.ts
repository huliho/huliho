// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Where the blobs of one account are served: the download template of
// its session and the account id that template takes (RFC 8620 section
// 6.2).
export interface DownloadSource {
  template: string;
  accountId: string;
}

// One blob as a request names it: the name its answer is filed under
// and the media type asked for.
export interface BlobRequest {
  blobId: string;
  name: string;
  type: string;
}

const HEX = 16;

// What encodeURIComponent leaves and a template value may not hold:
// everything outside the unreserved set is percent-encoded (RFC 6570
// section 3.2.2).
const LEFT_RESERVED = /[!'()*]/g;

// A value of one or two dots is a step in a path and no segment of it
// (RFC 3986 section 5.2.4), also when percent-encoded, so it travels as
// this instead.
const DOT_SEGMENT = /^\.\.?$/;
const DOTS_STAND_IN = "_";

// A name is sender text, so a lone surrogate is made well formed first.
function expanded(value: string): string {
  const kept = DOT_SEGMENT.test(value) ? DOTS_STAND_IN : value;
  return encodeURIComponent(kept.toWellFormed()).replace(
    LEFT_RESERVED,
    (character) => `%${character.charCodeAt(0).toString(HEX).toUpperCase()}`,
  );
}

// The URL of one blob: the template with its variables filled in.
export function downloadUrl(source: DownloadSource, blob: BlobRequest): string {
  const values = new Map([
    ["accountId", source.accountId],
    ["blobId", blob.blobId],
    ["name", blob.name],
    ["type", blob.type],
  ]);
  return source.template.replace(/\{(\w+)\}/g, (variable, name: string) => {
    const value = values.get(name);
    return value === undefined ? variable : expanded(value);
  });
}
