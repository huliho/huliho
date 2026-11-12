// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { carriedFiles, sentType, textLeaves } from "../compose/object";
import { callsFor, outcome } from "../jmap/calls";
import { JmapError } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import { z } from "../schema";
import type { DraftContent, DraftRow } from "./store";

const STRUCTURE_CALL = "p";

// One part of a tree as the read asks it: its blob, its type and the
// parts under it.
interface TreePart {
  blobId: string | null;
  type: string;
  subParts?: TreePart[] | null;
}

const treePartSchema: z.ZodMiniType<TreePart> = z.object({
  blobId: z.nullable(z.string()),
  type: z.string(),
  get subParts() {
    return z.exactOptional(z.nullable(z.array(treePartSchema)));
  },
});

const structureAnswerSchema = z.object({
  list: z.array(z.object({ bodyStructure: treePartSchema })),
});

function leavesOf(part: TreePart): TreePart[] {
  return part.subParts === undefined || part.subParts === null
    ? [part]
    : part.subParts.flatMap(leavesOf);
}

// The content with each file named by the blob of the server's copy where
// the row knows one, so a write reads the draft's own parts.
export function withParts(row: DraftRow): DraftContent {
  const parts = new Map(Object.entries(row.parts));
  return {
    ...row,
    attachments: row.attachments.map((file) => ({
      ...file,
      blobId: parts.get(file.blobId) ?? file.blobId,
    })),
  };
}

// The blob of each file in the new copy by the blob the composer named
// it with, read from the copy's tree, whose leaves after its text parts
// stand in the order the create gave them (RFC 8621 section 4.6). None
// where the tree does not match what was sent or the read does not
// reach the server: the next write then names the files as the composer
// did.
export async function partsOf(
  client: JmapClient,
  content: DraftContent,
  emailId: string,
): Promise<Record<string, string>> {
  const carried = carriedFiles(content);
  if (carried.length === 0) {
    return {};
  }
  const calls = callsFor((await client.session()).accountId);
  let responses;
  try {
    responses = await client.request([calls.structure(emailId, STRUCTURE_CALL)]);
  } catch (error) {
    if (error instanceof JmapError && error.code === "unavailable") {
      return {};
    }
    throw error;
  }
  const read = outcome(responses, STRUCTURE_CALL, structureAnswerSchema);
  const [email] = read.status === "ok" ? read.value.list : [];
  const leaves =
    email === undefined ? [] : leavesOf(email.bodyStructure).slice(textLeaves(content));
  const matched = carried.flatMap((file, index): [string, string][] => {
    const leaf = leaves.at(index);
    return leaf?.blobId !== null && leaf?.type.toLowerCase() === sentType(file)
      ? [[file.blobId, leaf.blobId]]
      : [];
  });
  return leaves.length === carried.length && matched.length === carried.length
    ? Object.fromEntries(matched)
    : {};
}
