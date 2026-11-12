// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { ACCOUNT, FakeJmap, UPSTREAM } from "../cache/fake-jmap";
import { JmapClient } from "./client";
import { UploadError, uploadFile } from "./upload";

const BYTES = 10;
const RETRY_SECONDS = 2;
const MS_PER_SECOND = 1000;

function serve(): { server: FakeJmap; client: JmapClient } {
  const server = new FakeJmap();
  vi.stubGlobal("fetch", server.fetch);
  vi.stubGlobal("XMLHttpRequest", server.UploadRequest);
  return { server, client: new JmapClient(ACCOUNT) };
}

function file(size = BYTES, type = "application/pdf"): Blob {
  return new Blob(["x".repeat(size)], { type });
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("a file goes to the account's upload route with its type and the header and answers its blob, rfc8620_6_1", async () => {
  const { server, client } = serve();
  const progress: [number, number][] = [];
  const answer = await uploadFile(client, file(), {
    progress: (sent, total) => {
      progress.push([sent, total]);
    },
  });
  expect(answer).toEqual({
    accountId: UPSTREAM,
    blobId: "upload-1",
    type: "application/pdf",
    size: BYTES,
  });
  const [seen] = server.uploaded;
  expect(seen?.url).toBe(`/api/jmap/${ACCOUNT}/upload/${UPSTREAM}`);
  expect(seen?.headers.get("content-type")).toBe("application/pdf");
  expect(seen?.headers.get("x-requested-with")).toBe("huliho");
  expect(progress).toEqual([
    [BYTES / 2, BYTES],
    [BYTES, BYTES],
  ]);
});

test("a file without a type goes up as octets", async () => {
  const { server, client } = serve();
  await uploadFile(client, file(BYTES, ""));
  expect(server.uploaded[0]?.headers.get("content-type")).toBe("application/octet-stream");
});

test("a file past the account's maxSizeUpload is refused before a byte leaves", async () => {
  const { server, client } = serve();
  server.uploadLimit = BYTES - 1;
  await expect(uploadFile(client, file())).rejects.toMatchObject({ code: "too_large" });
  expect(server.uploaded).toEqual([]);
});

test.each([
  [413, "too_large"],
  [507, "over_quota"],
  [408, "timeout"],
  [502, "unavailable"],
])("the route's %i reads as %s", async (status, code) => {
  const { server, client } = serve();
  server.uploadReplies.push({ status });
  const failed = uploadFile(client, file());
  await expect(failed).rejects.toBeInstanceOf(UploadError);
  await expect(failed).rejects.toMatchObject({ code });
});

test.each([
  ["unauthenticated", "unauthenticated"],
  ["upstream_credentials", "unavailable"],
  ["invalid_credentials", "unavailable"],
])("the route's 401 naming %s reads as %s", async (error, code) => {
  const { server, client } = serve();
  server.uploadReplies.push({ status: 401, error });
  const failed = uploadFile(client, file());
  await expect(failed).rejects.toBeInstanceOf(UploadError);
  await expect(failed).rejects.toMatchObject({ code });
});

test("a request the network drops or an answer that is no upload reads as unavailable", async () => {
  const { server, client } = serve();
  server.uploadReplies.push({ status: "dropped" });
  await expect(uploadFile(client, file())).rejects.toMatchObject({ code: "unavailable" });
  const odd = new FakeJmap();
  vi.stubGlobal("XMLHttpRequest", odd.UploadRequest);
  odd.uploadReplies.push({ status: 200 });
  await expect(uploadFile(client, file())).rejects.toMatchObject({ code: "unavailable" });
});

test("a busy lane is waited out as the Retry-After asks, a few times at most", async () => {
  const { server, client } = serve();
  vi.useFakeTimers();
  server.uploadReplies.push({ status: 429, retryAfter: String(RETRY_SECONDS) });
  const later = uploadFile(client, file());
  await vi.advanceTimersByTimeAsync(RETRY_SECONDS * MS_PER_SECOND - 1);
  expect(server.uploaded).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1);
  await vi.runAllTimersAsync();
  await expect(later).resolves.toMatchObject({ blobId: "upload-1" });
  expect(server.uploaded).toHaveLength(2);
  server.uploadReplies.push({ status: 429 }, { status: 429 }, { status: 429 });
  const busy = uploadFile(client, file()).catch((error: unknown) => error);
  await vi.runAllTimersAsync();
  expect(await busy).toMatchObject({ code: "busy" });
  expect(server.uploaded).toHaveLength(5);
});

test("an upload given up ends the request and rejects", async () => {
  const { server, client } = serve();
  server.holdUploads = true;
  const giving = new AbortController();
  const upload = uploadFile(client, file(), { signal: giving.signal });
  await vi.waitFor(() => {
    expect(server.heldUploads).toHaveLength(1);
  });
  giving.abort();
  await expect(upload).rejects.toMatchObject({ code: "aborted" });
  server.releaseUploads();
  expect(server.uploaded).toEqual([]);
  await expect(uploadFile(client, file(), { signal: giving.signal })).rejects.toMatchObject({
    code: "aborted",
  });
});
