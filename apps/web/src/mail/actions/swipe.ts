// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { useRef, useState } from "react";
import type { Dispatch, PointerEvent, RefObject, SetStateAction } from "react";

import { indexOfThread, rowAt } from "../use-thread-pages";

// How far a finger travels before the gesture reads as a swipe or as a
// scroll: the touch slop of the platforms (8 dp on Android).
const SLOP_PX = 8;
// Past this share of the row's width a release commits.
const SWIPE_COMMIT_RATIO = 0.5;
const REDUCED_MOTION = "(prefers-reduced-motion: reduce)";

// The way a finger moves a row, in the line's own direction.
export type SwipeWay = "toEnd" | "toStart";

// Under the finger, sliding home or sliding away after a commit; a row
// that slid away stays there until its thread leaves the list.
type SwipePhase = "held" | "home" | "away";

// The row a swipe moves: its thread and place, its offset in CSS pixels
// (positive to the right) and whether a release there commits.
export interface SwipeView {
  threadId: string;
  index: number;
  way: SwipeWay;
  offset: number;
  armed: boolean;
  phase: SwipePhase;
}

export interface SwipeInput {
  rows: ReadonlyMap<number, ListRow[]>;
  // The places of the rows the grid draws and the list's length: the rows
  // from a swiped row's place close up while one of them is drawn.
  drawn: readonly { index: number }[];
  rowCount: number;
  rowOf: (target: EventTarget) => number | null;
  // The ways that carry an action; a way without one keeps the row in place.
  ways: ReadonlySet<SwipeWay>;
  rtl: boolean;
  // Told when a touch turns into a swipe, so a pending long press ends.
  onEngage: () => void;
  // Runs the way's action on the row; resolves whether the thread left.
  onCommit: (way: SwipeWay, row: ListRow, index: number) => Promise<boolean>;
}

export interface Swipe {
  view: SwipeView | null;
  // Where a row that slid away stood once its thread left; the rows from
  // there close up. Null while none do.
  closing: number | null;
  onPointerDown: (event: PointerEvent<HTMLElement>) => void;
  onPointerMove: (event: PointerEvent<HTMLElement>) => void;
  onPointerUp: (event: PointerEvent<HTMLElement>) => void;
  onPointerCancel: (event: PointerEvent<HTMLElement>) => void;
  // Whether a click is the one a swipe leaves behind; true once.
  takeClick: () => boolean;
  // Told when the row's slide home has ended.
  onSettled: () => void;
  // Told when the rows have closed up.
  onClosed: () => void;
}

// The row's box once the touch is a swipe: its width and the edges a
// finger leaves it by.
interface Box {
  width: number;
  top: number;
  bottom: number;
}

// One finger on a row from its press until it lifts.
interface Track {
  pointerId: number;
  index: number;
  row: ListRow;
  x: number;
  y: number;
  // Read as a scroll: the rest of the gesture is the list's.
  scroll: boolean;
  // Null until the touch is a swipe.
  box: Box | null;
  way: SwipeWay;
  offset: number;
  armed: boolean;
}

function wayOf(dx: number, rtl: boolean): SwipeWay {
  return dx > 0 !== rtl ? "toEnd" : "toStart";
}

// A touch keeps its first target, so the row's box tells where the finger is.
function boxOf(target: EventTarget): Box | null {
  const row = target instanceof Element ? target.closest("[data-index]") : null;
  if (row === null) {
    return null;
  }
  const { width, top, bottom } = row.getBoundingClientRect();
  return width > 0 ? { width, top, bottom } : null;
}

function motionReduced(): boolean {
  return window.matchMedia(REDUCED_MOTION).matches;
}

function homeOf(view: SwipeView | null): SwipeView | null {
  if (view === null || view.offset === 0 || motionReduced()) {
    return null;
  }
  return { ...view, offset: 0, armed: false, phase: "home" };
}

// Whether a drawn row from `from` on, short of the list's end, carries the
// close-up; only such a row plays it to its end.
function marked(input: SwipeInput, from: number): boolean {
  return input.drawn.some((item) => item.index >= from && item.index < input.rowCount);
}

// Moves the track by a pointer's position; false while the gesture is
// not a swipe or once it is the list's scroll.
function follow(track: Track, input: SwipeInput, event: PointerEvent<HTMLElement>): boolean {
  const dx = event.clientX - track.x;
  const dy = event.clientY - track.y;
  if (track.box === null) {
    if (Math.abs(dy) >= SLOP_PX && Math.abs(dy) >= Math.abs(dx)) {
      track.scroll = true;
      return false;
    }
    if (Math.abs(dx) < SLOP_PX) {
      return false;
    }
    track.box = boxOf(event.target);
    if (track.box === null) {
      track.scroll = true;
      return false;
    }
    input.onEngage();
  }
  track.way = wayOf(dx, input.rtl);
  const reach = input.ways.has(track.way) ? Math.min(Math.abs(dx), track.box.width) : 0;
  track.offset = Math.sign(dx) * reach;
  track.armed = reach >= track.box.width * SWIPE_COMMIT_RATIO;
  return true;
}

function outside(box: Box | null, event: PointerEvent<HTMLElement>): boolean {
  return box !== null && (event.clientY < box.top || event.clientY > box.bottom);
}

// What the handlers share: the input, the finger on its row, whether a
// click is the swipe's own and the view the row draws.
interface Gesture {
  input: SwipeInput;
  track: RefObject<Track | null>;
  swallow: RefObject<boolean>;
  setView: Dispatch<SetStateAction<SwipeView | null>>;
}

// A track whose touch moved its row.
type Moving = Track & { box: Box };

// The finger the event belongs to, let go of, once it moved a row; null
// for any other.
function release(gesture: Gesture, event: PointerEvent<HTMLElement>): Moving | null {
  const held = gesture.track.current;
  if (held === null || held.pointerId !== event.pointerId) {
    return null;
  }
  gesture.track.current = null;
  const { box } = held;
  return box === null ? null : { ...held, box };
}

function back(gesture: Gesture): void {
  gesture.setView(homeOf);
}

// A primary touch on a loaded row, where a way carries an action.
function startOf(input: SwipeInput, event: PointerEvent<HTMLElement>): Track | null {
  const index = input.rowOf(event.target);
  const row = index === null ? undefined : rowAt(input.rows, index);
  if (event.pointerType !== "touch" || !event.isPrimary || input.ways.size === 0) {
    return null;
  }
  if (index === null || row === undefined) {
    return null;
  }
  return {
    pointerId: event.pointerId,
    index,
    row,
    x: event.clientX,
    y: event.clientY,
    scroll: false,
    box: null,
    way: "toEnd",
    offset: 0,
    armed: false,
  };
}

// A touch on a row starts a track; a second finger sends a moving row home.
function press(gesture: Gesture, event: PointerEvent<HTMLElement>): void {
  gesture.swallow.current = false;
  const held = gesture.track.current;
  gesture.track.current = null;
  if (held !== null && held.box !== null) {
    back(gesture);
    return;
  }
  gesture.track.current = startOf(gesture.input, event);
}

// The row follows the finger along its line; a finger that leaves the
// row across its top or bottom edge sends it home.
function move(gesture: Gesture, event: PointerEvent<HTMLElement>): void {
  const held = gesture.track.current;
  if (held === null || held.scroll || held.pointerId !== event.pointerId) {
    return;
  }
  if (outside(held.box, event)) {
    gesture.track.current = null;
    gesture.swallow.current = true;
    back(gesture);
    return;
  }
  if (follow(held, gesture.input, event)) {
    const { row, index, way, offset, armed } = held;
    gesture.setView({ threadId: row.threadId, index, way, offset, armed, phase: "held" });
  }
}

// Past the commit point the row slides away and the way's action runs; a
// change that took nothing sends it home.
function lift(gesture: Gesture, event: PointerEvent<HTMLElement>): void {
  const held = release(gesture, event);
  if (held === null) {
    return;
  }
  gesture.swallow.current = true;
  if (!held.armed) {
    back(gesture);
    return;
  }
  const { row, index, way } = held;
  const offset = Math.sign(held.offset) * held.box.width;
  gesture.setView({ threadId: row.threadId, index, way, offset, armed: true, phase: "away" });
  void gesture.input.onCommit(way, row, index).then((left) => {
    if (!left) {
      back(gesture);
    }
    return left;
  });
}

// A touch that travels sideways on a row moves it; a release past the
// commit point runs the way's action and one short of it sends the row
// home. A scroll, a second finger, a cancel or the finger leaving the row
// ends a swipe with nothing run.
export function useSwipe(input: SwipeInput): Swipe {
  const [view, setView] = useState<SwipeView | null>(null);
  const [closing, setClosing] = useState<number | null>(null);
  const track = useRef<Track | null>(null);
  const swallow = useRef(false);
  if (view?.phase === "away" && indexOfThread(input.rows, view.threadId) === null) {
    setView(null);
    if (marked(input, view.index) && !motionReduced()) {
      setClosing(view.index);
    }
  }
  // The close-up ends on a drawn row; once none carries it, nothing will.
  if (closing !== null && !marked(input, closing)) {
    setClosing(null);
  }
  const gesture: Gesture = { input, track, swallow, setView };
  return {
    view,
    closing,
    onPointerDown: (event) => {
      press(gesture, event);
    },
    onPointerMove: (event) => {
      move(gesture, event);
    },
    onPointerUp: (event) => {
      lift(gesture, event);
    },
    onPointerCancel: (event) => {
      if (release(gesture, event) !== null) {
        back(gesture);
      }
    },
    takeClick: () => {
      const taken = swallow.current;
      swallow.current = false;
      return taken;
    },
    onSettled: () => {
      setView((current) => (current?.phase === "home" ? null : current));
    },
    onClosed: () => {
      setClosing(null);
    },
  };
}
