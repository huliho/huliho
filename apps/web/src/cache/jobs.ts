// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { jobRound, jobStarted, runJob } from "@huliho/core";
import type { Job, JobRound, JmapClient, MailStore } from "@huliho/core";

import type { CacheMessage } from "./messages";
import { attempt } from "./outcome";
import { moved } from "./writes";

// What a job asks of the coordinator: an account's client, its lock, the
// channel to the tabs and a poll of the account once the job ends.
export interface JobHost {
  client(accountId: string): JmapClient;
  locked<Value>(accountId: string, run: (store: MailStore) => Promise<Value>): Promise<Value>;
  post(message: CacheMessage): void;
  poll(accountId: string): void;
}

interface Running {
  canceled: boolean;
}

// The folder jobs, one per account at a time. Each round holds the
// account's lock for itself alone, so a poll and a flush interleave; the
// tabs hear the progress after every round. A job outlives the tab that
// started it, and the account polls once it ends, since a round lands
// the counts of every mailbox while the other lists come with the poll.
export class Jobs {
  private readonly host: JobHost;
  private readonly running = new Map<string, Running>();

  constructor(host: JobHost) {
    this.host = host;
  }

  // A job asked while one of the account runs is left alone; the tabs
  // see the running one's progress. Every tab hears a job start before
  // its first round.
  start(accountId: string, job: Job): void {
    if (this.running.has(accountId)) {
      return;
    }
    const run: Running = { canceled: false };
    this.running.set(accountId, run);
    this.host.post({ kind: "job", accountId, progress: jobStarted(job) });
    void this.run(accountId, job, run);
  }

  // The job ends after the round that runs.
  cancel(accountId: string): void {
    const run = this.running.get(accountId);
    if (run !== undefined) {
      run.canceled = true;
    }
  }

  private async run(accountId: string, job: Job, run: Running): Promise<void> {
    const client = this.host.client(accountId);
    const outcome = await attempt(() =>
      runJob(jobStarted(job), {
        round: (progress) =>
          this.host.locked(accountId, (store) => jobRound(client, store, progress)),
        canceled: () => run.canceled,
        told: (round) => {
          this.tell(accountId, round);
        },
      }),
    );
    if (!outcome.ok) {
      console.error("cache: a folder job failed", outcome.failure.code);
    }
    this.running.delete(accountId);
    this.host.poll(accountId);
  }

  private tell(accountId: string, { progress, changes }: JobRound): void {
    if (moved(changes)) {
      this.host.post({ kind: "changed", accountId, ...changes });
    }
    this.host.post({ kind: "job", accountId, progress });
  }
}
