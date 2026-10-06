// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { BLANK_PIXEL } from "./css";
import { FRAME_CSP, buildFrameDocument } from "./frame-document";
import { DOWNLOAD_PREFIX, OPTIONS, PROXY_PREFIX, built, message, part } from "./frame-rig";
import { BLOCKED_BOXES_MAX } from "./images";

const LOGO = part("image/png", {
  partId: "3",
  blobId: "b-logo",
  name: "logo.png",
  cid: "logo@shop.example",
});
const TURNED = "calc(1 - l) c h / alpha)";

function page(html: string): Document {
  return new DOMParser().parseFromString(html, "text/html");
}

// Whether the frame adds room around the mail.
function padded(html: string): boolean {
  const style = built(html).page.head.querySelector("style")?.textContent ?? "";
  return style.includes("padding: 16px;");
}

test("the document carries the frame's policy, no referrer, no prefetch and the body in its own direction", () => {
  const { html, page: read } = built("<p>hi</p>");
  expect(html.startsWith("<!doctype html><html><head>")).toBe(true);
  const metas = Array.from(read.head.querySelectorAll("meta"), (meta) => [
    meta.getAttribute("charset") ?? meta.getAttribute("http-equiv") ?? meta.getAttribute("name"),
    meta.getAttribute("content"),
  ]);
  expect(metas).toEqual([
    ["utf-8", null],
    ["Content-Security-Policy", FRAME_CSP],
    ["referrer", "no-referrer"],
    ["x-dns-prefetch-control", "off"],
  ]);
  expect(FRAME_CSP).toBe(
    "default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'",
  );
  expect(read.head.querySelectorAll("style")).toHaveLength(1);
  expect(read.head.querySelectorAll("script, link, base")).toHaveLength(0);
  expect(read.body.getAttribute("dir")).toBe("auto");
  expect(read.body.innerHTML).toBe("<p>hi</p>");
});

test("the base style makes the root as tall as its content and takes the card's type", () => {
  const style = built("<p>hi</p>").page.head.querySelector("style")?.textContent ?? "";
  expect(style).toContain("html, body { margin: 0; height: auto !important; }");
  expect(style).toContain('font-family: "Hanken Grotesk", Arial, sans-serif; font-size: 13px;');
  expect(style).toContain("line-height: 1.45; overflow-x: auto; overflow-y: hidden;");
  expect(style).toContain("img { max-width: 100%; }");
});

test("a value from the card cannot end the base style or its element", () => {
  const style = {
    ...OPTIONS.style,
    fontFamily: 'x; } </style><script>top.__x=1</script> p { color: "red',
  };
  const { page: read } = built("<p>hi</p>", { style });
  expect(read.querySelector("script")).toBeNull();
  expect(read.head.querySelectorAll("style")).toHaveLength(1);
  expect(read.body.innerHTML).toBe("<p>hi</p>");
});

test("the parts of the body list render in order: HTML, an image, text as text", () => {
  const photo = part("image/jpeg", { partId: "2", blobId: "b-photo", name: "photo 1.jpg" });
  const footer = part("text/plain", { partId: "9" });
  const body = message(["<p>first</p>", photo, "<p>second</p>", footer, part("audio/mpeg")]);
  body.bodyValues["9"] = {
    value: "-- \n<b>list</b> footer & more",
    isEncodingProblem: false,
    isTruncated: false,
  };
  const frame = buildFrameDocument(body, OPTIONS);
  expect(page(frame.html).body.innerHTML).toBe(
    "<p>first</p>" +
      `<img src="${DOWNLOAD_PREFIX}b-photo/photo%201.jpg?type=image%2Fjpeg" alt="photo 1.jpg">` +
      "<p>second</p>" +
      '<div style="white-space: pre-wrap;">-- \n&lt;b&gt;list&lt;/b&gt; footer &amp; more</div>',
  );
  // The image is shown inline, so the strip leaves it out.
  expect(Array.from(frame.inlineParts)).toEqual(["2"]);
});

test("a part without a value and one of another kind render nothing", () => {
  const body = message([part("text/html", { partId: "7" }), part("application/pdf")]);
  expect(page(buildFrameDocument(body, OPTIONS).html).body.innerHTML).toBe("");
});

test("a cid image resolves inside the message and nowhere else", () => {
  const html =
    '<img src="cid:logo@shop.example" width="120"><img src="cid:other@message.example" alt="x">' +
    '<img src="cid:doc@shop.example">';
  const attached = part("text/html", { blobId: "b-doc", cid: "doc@shop.example" });
  const body = message([html], [LOGO, attached]);
  const frame = buildFrameDocument(body, OPTIONS);
  const images = page(frame.html).body.querySelectorAll("img");
  expect(Array.from(images, (image) => image.getAttribute("src"))).toEqual([
    `${DOWNLOAD_PREFIX}b-logo/logo.png?type=image%2Fpng`,
    null,
    null,
  ]);
  expect(images[0]?.getAttribute("width")).toBe("120");
  expect(images[1]?.getAttribute("alt")).toBe("x");
  // The part shown inline is the one the strip leaves out.
  expect(Array.from(frame.inlineParts)).toEqual(["3"]);
});

test("a remote image is a box with its alt text until the reader allows the sender", () => {
  const html =
    '<img src="https://cdn.example/hero.png" width="600" height="180" alt="Autumn &amp; <sale>">';
  const blocked = built(html);
  const image = blocked.page.body.querySelector("img");
  const src = image?.getAttribute("src") ?? "";
  expect(src.startsWith("data:image/svg+xml,")).toBe(true);
  const svg = decodeURIComponent(src.slice("data:image/svg+xml,".length));
  expect(svg).toContain("Autumn &amp; &lt;sale&gt;</text>");
  expect(svg).toContain('fill="rgb(242, 245, 246)" stroke="rgb(223, 230, 232)"');
  const box = new DOMParser().parseFromString(svg, "image/svg+xml");
  expect(box.querySelector("parsererror")).toBeNull();
  expect(box.querySelector("text")?.getAttribute("font-family")).toBe(OPTIONS.style.fontFamily);
  expect([image?.getAttribute("width"), image?.getAttribute("height")]).toEqual(["600", "180"]);
  expect(image?.getAttribute("alt")).toBe("Autumn & <sale>");
  expect(blocked.remote).toBe(1);
  expect(blocked.html).not.toContain("cdn.example");
  const allowed = built(html, { remote: true });
  expect(allowed.page.body.querySelector("img")?.getAttribute("src")).toBe(
    `${PROXY_PREFIX}https%3A%2F%2Fcdn.example%2Fhero.png`,
  );
  expect(allowed.remote).toBe(1);
});

// Remote images numbered from `from`, each with a text of its own.
function remoteImages(from: number, count: number): string {
  return Array.from({ length: count }, (_, index) => {
    const n = String(from + index);
    return `<img src="https://cdn.example/${n}.png" width="40" height="20" alt="n${n}">`;
  }).join("");
}

test("a mail of more remote images than the bound draws that many boxes and keeps the rest blank, across its parts", () => {
  const extra = 5;
  const half = BLOCKED_BOXES_MAX / 2;
  const parts = message([remoteImages(0, half), remoteImages(half, half + extra)]);
  const { html, remote } = buildFrameDocument(parts, OPTIONS);
  const read = page(html);
  const sources = Array.from(read.body.querySelectorAll("img"), (image) => image.src);
  expect(sources.filter((src) => src.startsWith("data:image/svg+xml,"))).toHaveLength(
    BLOCKED_BOXES_MAX,
  );
  expect(sources.slice(BLOCKED_BOXES_MAX)).toEqual(
    Array.from({ length: extra }, () => BLANK_PIXEL),
  );
  // The blank ones keep their place and still count for the bar.
  expect(read.body.querySelectorAll('img[width="40"][height="20"]')).toHaveLength(
    BLOCKED_BOXES_MAX + extra,
  );
  expect(remote).toBe(BLOCKED_BOXES_MAX + extra);
});

test("an alt text is cut to its bound and loses what XML cannot hold", () => {
  const alt = `${"a".repeat(100)}\u0001`;
  const src = built(`<img src="https://cdn.example/a.png" alt="${alt}">`)
    .page.body.querySelector("img")
    ?.getAttribute("src");
  const svg = decodeURIComponent((src ?? "").slice("data:image/svg+xml,".length));
  expect(svg).toContain(`>${"a".repeat(80)}…</text>`);
  expect(
    new DOMParser().parseFromString(svg, "image/svg+xml").querySelector("parsererror"),
  ).toBeNull();
});

test("the remote count covers images, CSS and background attributes alike", () => {
  const html =
    "<style>.a { background-image: url(https://cdn.example/1.png) }</style>" +
    '<img src="https://cdn.example/2.png"><table background="https://cdn.example/3.png"><tr><td ' +
    'style="background-image: url(https://cdn.example/4.png)">x</td></tr></table>' +
    '<img src="https://cdn.example/2.png"><img src="data:image/png;base64,AAAA"><img src="logo.png">';
  const blocked = built(html);
  expect(blocked.remote).toBe(4);
  expect(blocked.html).not.toContain("cdn.example");
  expect(blocked.page.body.querySelector("table")?.hasAttribute("background")).toBe(false);
  const allowed = built(html, { remote: true });
  expect(allowed.remote).toBe(4);
  expect(allowed.page.body.querySelector("table")?.getAttribute("background")).toBe(
    `${PROXY_PREFIX}https%3A%2F%2Fcdn.example%2F3.png`,
  );
  expect(allowed.html.match(/cdn\.example/g)).toHaveLength(5);
  expect(allowed.html).not.toContain('"https://cdn.example');
});

test("a URL on the instance's own host and a relative one load nothing", () => {
  const { page: read, remote } = built(
    '<img src="https://mail.example.test/logo.png"><img src="/api/remote-image?url=https://evil.example/p.gif">' +
      '<img src="logo.png"><table background="/api/x"><tr><td>x</td></tr></table>',
  );
  expect(read.body.querySelectorAll("[src], [background]")).toHaveLength(0);
  expect(remote).toBe(0);
});

test("a style attribute is written back as the engine read it", () => {
  const { page: read } = built(
    '<p style="color: red; width: expression(top.__x=1); behavior: url(https://evil.example/x.htc)">x</p><p style="height: 100vh">y</p>',
  );
  const [first, second] = Array.from(read.body.querySelectorAll("p"));
  expect(first?.getAttribute("style")).toBe("color: red;");
  expect(second?.hasAttribute("style")).toBe(false);
});

test("a top element loses a height in percent and keeps one in pixels", () => {
  const { page: read } = built(
    '<table height="100%" style="height: 100%; min-height: 100%; width: 100%"><tr><td height="100%">x</td></tr></table><div height="300" style="height: 300px">y</div>',
  );
  const table = read.body.querySelector("table");
  expect(table?.hasAttribute("height")).toBe(false);
  expect(table?.getAttribute("style")).toBe("width: 100%;");
  expect(read.body.querySelector("td")?.getAttribute("height")).toBe("100%");
  expect(read.body.querySelector("div")?.getAttribute("style")).toBe("height: 300px;");
});

test("a mail that paints no background gets room around it and one that paints its own gets none", () => {
  expect(padded("<p>plain</p>")).toBe(true);
  expect(padded("<p>one</p><p>two</p>")).toBe(true);
  expect(padded('<table bgcolor="#eeeeee"><tr><td>x</td></tr></table>')).toBe(false);
  expect(padded('<div style="background-color: #eeeeee">x</div>')).toBe(false);
  expect(padded('<div style="background-color: transparent">x</div>')).toBe(true);
  expect(padded("<style>body { background-color: #eeeeee }</style><p>x</p>")).toBe(false);
  expect(
    padded('<style>.wrap { background-color: #eeeeee }</style><div class="wrap">x</div>'),
  ).toBe(false);
  expect(
    padded('<style>.cell { background-color: #eeeeee }</style><div><p class="cell">x</p></div>'),
  ).toBe(true);
});

test("the light theme shows a mail on the card's surface, with the app's text color", () => {
  const frame = built("<p>x</p>", { theme: "light", adapt: true });
  const style = frame.page.head.querySelector("style")?.textContent ?? "";
  expect(style).toContain("color-scheme: light; color: rgb(35, 43, 47);");
  expect(style).not.toContain("background:");
  expect(frame.adapted).toBe(false);
  expect(frame.declaresDark).toBe(false);
});

test("the dark theme adapts a light-only mail: its colors turned, the canvas the card's", () => {
  const html =
    "<style>.t { color: #232b2f; background-color: #ffffff }</style>" +
    '<table bgcolor="FFFFFF"><tr><td class="t" style="border: 1px solid #cccccc"><font color="#333333">x</font>' +
    '<img src="data:image/png;base64,AAAA"></td></tr></table>';
  const frame = built(html, { theme: "dark", adapt: true });
  expect(frame.adapted).toBe(true);
  expect(frame.declaresDark).toBe(false);
  const head = frame.page.head.querySelector("style")?.textContent ?? "";
  expect(head).toContain(
    "color-scheme: dark; background: rgb(255, 255, 255); color: rgb(35, 43, 47);",
  );
  const sheet = frame.page.body.querySelector("style")?.textContent ?? "";
  expect(sheet).toContain(`color: oklch(from rgb(35, 43, 47) ${TURNED}`);
  expect(sheet).toContain(`background-color: oklch(from rgb(255, 255, 255) ${TURNED}`);
  expect(frame.page.body.querySelector("td")?.getAttribute("style")).toContain(
    `oklch(from rgb(204, 204, 204) ${TURNED}`,
  );
  expect(frame.page.body.querySelector("table")?.getAttribute("style")).toBe(
    `background-color: oklch(from rgb(255, 255, 255) ${TURNED};`,
  );
  expect(frame.page.body.querySelector("[color]")?.getAttribute("style")).toBe(
    `color: oklch(from rgb(51, 51, 51) ${TURNED};`,
  );
  expect(frame.page.body.querySelector("img")?.getAttribute("src")).toBe(
    "data:image/png;base64,AAAA",
  );
});

test("showing the original colors is the same document as a mail never adapted", () => {
  const html = '<p style="color: #232b2f">x</p>';
  const original = built(html, { theme: "dark", adapt: false });
  expect(original.adapted).toBe(false);
  expect(original.page.body.innerHTML).toBe(built(html).page.body.innerHTML);
  const head = original.page.head.querySelector("style")?.textContent ?? "";
  expect(head).toContain("color-scheme: light; background: #ffffff; color: #000000;");
});

test("a mail that declares dark styles keeps its own rules under both settings", () => {
  const html =
    "<style>p { color: #232b2f } @media (prefers-color-scheme: dark) { p { color: #e4eaec } }</style><p>x</p>";
  const adapt = built(html, { theme: "dark", adapt: true });
  const asSent = built(html, { theme: "dark", adapt: false });
  expect(adapt.declaresDark).toBe(true);
  expect(adapt.adapted).toBe(false);
  expect(adapt.html).toBe(asSent.html);
  expect(adapt.html).not.toContain("oklch(");
  expect(adapt.page.body.querySelector("style")?.textContent).toContain("@media (min-width: 0px)");
  const head = adapt.page.head.querySelector("style")?.textContent ?? "";
  expect(head).toContain("color-scheme: dark; background: rgb(255, 255, 255);");
  const light = built(html, { theme: "light", adapt: true });
  expect(light.page.body.querySelector("style")?.textContent).toContain(
    "@media (not (min-width: 0px))",
  );
});

test("a scheme the mail sets in a style attribute declares dark styles too", () => {
  const frame = built('<div style="color-scheme: light dark">x</div>', {
    theme: "dark",
    adapt: true,
  });
  expect(frame.declaresDark).toBe(true);
  expect(frame.adapted).toBe(false);
  expect(frame.page.body.querySelector("div")?.getAttribute("style")).toBe("color-scheme: dark;");
});

test("every link of every part goes through the open route", () => {
  const body = message([
    '<a href="https://a.example/">a</a>',
    '<a href="https://b.example/">b</a>',
  ]);
  const read = page(buildFrameDocument(body, OPTIONS).html);
  const hrefs = Array.from(read.body.querySelectorAll("a"), (link) => link.getAttribute("href"));
  expect(hrefs).toHaveLength(2);
  expect(hrefs.every((href) => href?.startsWith("/open#k=key-1&u=https%3A%2F%2F") === true)).toBe(
    true,
  );
});

test("the same message builds the same document every time", () => {
  const body = message(['<style>p { color: red }</style><p style="color: blue">x</p>'], [LOGO]);
  expect(buildFrameDocument(body, OPTIONS).html).toBe(buildFrameDocument(body, OPTIONS).html);
});
