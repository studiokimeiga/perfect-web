// Track `upload-metadata` (ADR-XXXX, amending ADR-0260): an uploaded image
// is kept and served without what names a place, a device, a person or a
// time, in three engines. On the uploads track's hosts (`UPLOADS_PORTS`,
// each with a blob storage of its own), each file is built here from a real
// encoder's fixture (`e2e/uploads/`) with metadata put in it:
//   - a phone's JPEG (Exif with its camera, time and GPS position, turned a
//     quarter; XMP naming its creator; IPTC's by-line; a comment; a second
//     image after its EOI), a PNG (tEXt, iTXt's XMP, tIME, an eXIf turned a
//     quarter), a WebP (EXIF turned a quarter and XMP, flagged in VP8X), an
//     animated WebP (its EXIF), and a GIF (a comment, XMP's application
//     extension): each is attached and posted, shown in the feed decoded at
//     the size its markup states, and served, at the key of the bytes kept,
//     without any of the known values;
//   - the JPEG keeps its turn: stored 8 by 5 under `Orientation` 6, it is
//     shown 5 by 8, its stored left column its shown top row (a canvas reads
//     its corners), so a portrait photo stays upright;
//   - EVIDENCE lines record what each engine shows of the files as they were
//     sent, read from a blank page: whether it turns a PNG by its eXIf, a
//     WebP by its EXIF, and which of a JPEG's two Exif structures it reads.
import { expect, test } from "@playwright/test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { crc32 } from "node:zlib";
import { UPLOADS_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${UPLOADS_PORTS[testInfo.project.name]}`);
  },
});

// One reader's lease at a time on a host: its tests in order.
test.describe.configure({ mode: "serial" });

const fixture = (name) => readFileSync(new URL(`./uploads/${name}`, import.meta.url));
const latin1 = (text) => Buffer.from(text, "latin1");
const u16be = (v) => Buffer.from([v >> 8, v & 0xff]);
const u32 = (v, little = false) => {
  const b = Buffer.alloc(4);
  little ? b.writeUInt32LE(v) : b.writeUInt32BE(v);
  return b;
};

// ---- What names a place, a device, a person or a time: known values, each
// searched for in what is served. ----

/** 48° 51′ 29.61″: GPSLatitude's three RATIONALs, big-endian. */
const LATITUDE = Buffer.concat([48, 1, 51, 1, 2961, 100].map((v) => u32(v)));
const XMP_LATITUDE = latin1("48,51.4935N");
const CAMERA = latin1("PlerisPhone 7 Pro");
const TAKEN = latin1("2026:10:25 09:41:00");
const NAME = latin1("Jane Roe");
const COMMENT = latin1("Taken at 12 Rue Example");
const KNOWN = {
  "a GPS latitude": LATITUDE,
  "an XMP latitude": XMP_LATITUDE,
  "a camera": CAMERA,
  "when it was taken": TAKEN,
  "a person's name": NAME,
  "a comment": COMMENT,
};
const identified = (bytes) => Object.keys(KNOWN).filter((what) => bytes.includes(KNOWN[what]));

/** A TIFF IFD at offset `at` (big-endian): its entries, no next IFD, then each value over four bytes. */
function ifd(at, entries) {
  const head = [u16be(entries.length)];
  const values = [];
  let past = at + 2 + 12 * entries.length + 4;
  for (const [tag, type, count, value] of entries) {
    head.push(u16be(tag), u16be(type), u32(count));
    if (value.length <= 4) {
      head.push(Buffer.concat([value, Buffer.alloc(4 - value.length)]));
    } else {
      head.push(u32(past));
      const padded = value.length % 2 ? Buffer.concat([value, Buffer.alloc(1)]) : value;
      values.push(padded);
      past += padded.length;
    }
  }
  return Buffer.concat([...head, u32(0), ...values]);
}

/** The Exif a phone writes: IFD0's Make, Orientation, DateTime and GPSInfo; its GPS IFD's latitude. */
function tiff(orientation) {
  const ascii = (b) => Buffer.concat([b, Buffer.alloc(1)]);
  const ifd0 = (gps) =>
    ifd(8, [
      [0x010f, 2, CAMERA.length + 1, ascii(CAMERA)],
      [0x0112, 3, 1, u16be(orientation)],
      [0x0132, 2, TAKEN.length + 1, ascii(TAKEN)],
      [0x8825, 4, 1, u32(gps)],
    ]);
  const gps = 8 + ifd0(0).length;
  return Buffer.concat([latin1("MM\0\x2a"), u32(8), ifd0(gps), ifd(gps, [[1, 2, 2, latin1("N\0")], [2, 5, 3, LATITUDE]])]);
}

const XMP = Buffer.concat([
  latin1(
    '<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">' +
      '<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" ' +
      'xmlns:exif="http://ns.adobe.com/exif/1.0/" exif:GPSLatitude="',
  ),
  XMP_LATITUDE,
  latin1('"><dc:creator><rdf:Seq><rdf:li>'),
  NAME,
  latin1("</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>"),
]);

// ---- Each format, as an editor or a phone writes it. ----

const segment = (marker, data) => Buffer.concat([Buffer.from([0xff, marker]), u16be(data.length + 2), data]);
const exifSegment = (orientation) => segment(0xe1, Buffer.concat([latin1("Exif\0\0"), tiff(orientation)]));

/** eight-by-five.jpeg as a phone writes it: Exif turned a quarter (6), XMP, IPTC, a comment, a second image after EOI. */
function phoneJpeg() {
  const base = fixture("eight-by-five.jpeg");
  const iim = Buffer.concat([Buffer.from([0x1c, 0x02, 0x50]), u16be(NAME.length), NAME]);
  const iptc = Buffer.concat([latin1("Photoshop 3.0\x008BIM"), u16be(0x0404), Buffer.from([0, 0]), u32(iim.length), iim]);
  const second = Buffer.concat([Buffer.from([0xff, 0xd8]), exifSegment(1), Buffer.from([0xff, 0xd9])]);
  return Buffer.concat([
    base.subarray(0, 2),
    exifSegment(6),
    segment(0xe1, Buffer.concat([latin1("http://ns.adobe.com/xap/1.0/\0"), XMP])),
    segment(0xed, iptc),
    segment(0xfe, COMMENT),
    base.subarray(2),
    second,
  ]);
}

/** eight-by-five.jpeg with two Exif structures, turned (6) then not (1). */
function twiceExifJpeg() {
  const base = fixture("eight-by-five.jpeg");
  return Buffer.concat([base.subarray(0, 2), exifSegment(6), exifSegment(1), base.subarray(2)]);
}

const pngChunk = (type, data) => {
  const body = Buffer.concat([latin1(type), data]);
  return Buffer.concat([u32(data.length), body, u32(crc32(body))]);
};

/** six-by-four.png as an editor writes it: a comment, XMP, a time, and an eXIf turned a quarter (6). */
function editedPng() {
  const base = fixture("six-by-four.png");
  return Buffer.concat([
    base.subarray(0, 33),
    pngChunk("tEXt", Buffer.concat([latin1("Comment\0"), COMMENT])),
    pngChunk("iTXt", Buffer.concat([latin1("XML:com.adobe.xmp\0\0\0\0\0"), XMP])),
    pngChunk("tIME", Buffer.from([0x07, 0xea, 10, 25, 9, 41, 0])),
    pngChunk("eXIf", tiff(6)),
    base.subarray(33),
  ]);
}

const riffChunk = (fourcc, data) =>
  Buffer.concat([latin1(fourcc), u32(data.length, true), data, Buffer.alloc(data.length % 2)]);
const riff = (chunks) => {
  const body = Buffer.concat(chunks);
  return Buffer.concat([latin1("RIFF"), u32(body.length + 4, true), latin1("WEBP"), body]);
};

/** twelve-by-nine-alpha.webp with EXIF turned a quarter (6) and XMP, flagged in its VP8X. */
function editedWebp() {
  const base = Buffer.from(fixture("twelve-by-nine-alpha.webp"));
  base[20] |= 0x08 | 0x04;
  return riff([base.subarray(12), riffChunk("EXIF", tiff(6)), riffChunk("XMP ", XMP)]);
}

/** Two frames of seven-by-three-lossless.webp's image, looping, and an EXIF. */
function animatedWebp() {
  const vp8l = fixture("seven-by-three-lossless.webp").subarray(12);
  const frame = riffChunk("ANMF", Buffer.concat([Buffer.from([0, 0, 0, 0, 0, 0, 6, 0, 0, 2, 0, 0, 100, 0, 0, 0]), vp8l]));
  return riff([
    riffChunk("VP8X", Buffer.from([0x02 | 0x08, 0, 0, 0, 6, 0, 0, 2, 0, 0])),
    riffChunk("ANIM", Buffer.from([255, 255, 255, 255, 0, 0])),
    frame,
    frame,
    riffChunk("EXIF", tiff(1)),
  ]);
}

const subBlocks = (data) => {
  const parts = [];
  for (let at = 0; at < data.length; at += 255) {
    const piece = data.subarray(at, at + 255);
    parts.push(Buffer.from([piece.length]), piece);
  }
  return Buffer.concat([...parts, Buffer.from([0])]);
};

/** five-by-nine.gif as GIF89a, with a comment and XMP's application extension. */
function editedGif() {
  const base = fixture("five-by-nine.gif");
  return Buffer.concat([
    latin1("GIF89a"),
    base.subarray(6, 205),
    Buffer.from([0x21, 0xfe]),
    subBlocks(COMMENT),
    Buffer.from([0x21, 0xff, 11]),
    latin1("XMP DataXMP"),
    subBlocks(XMP),
    base.subarray(205),
  ]);
}

const FILES = [
  { name: "IMG_0001.jpg", kind: "jpeg", mime: "image/jpeg", bytes: phoneJpeg(), shown: [5, 8] },
  { name: "edited.png", kind: "png", mime: "image/png", bytes: editedPng(), shown: [6, 4] },
  { name: "edited.webp", kind: "webp", mime: "image/webp", bytes: editedWebp(), shown: [12, 9] },
  { name: "animated.webp", kind: "webp", mime: "image/webp", bytes: animatedWebp(), shown: [7, 3] },
  { name: "edited.gif", kind: "gif", mime: "image/gif", bytes: editedGif(), shown: [5, 9] },
];

// ---- The feed. ----

/** The home page, its handlers attached. */
async function home(page) {
  await page.goto("/");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** Attach `file` with the home page's form, and post it with `text`. */
async function attachAndPost(page, file, text) {
  await page.locator("#attach input[type=file]").setInputFiles({ name: file.name, mimeType: file.mime, buffer: file.bytes });
  await Promise.all([
    page.waitForNavigation(),
    page.locator("#attach").getByRole("button", { name: "Attach", exact: true }).click(),
  ]);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await page.getByLabel("What's happening?").fill(text);
  await page.getByLabel("The image, in words, for whoever cannot see it").fill(`${file.name}, in words`);
  await page.getByRole("button", { name: "Post", exact: true }).click();
  return page.locator("main > ul > li").filter({ hasText: text }).locator("img.photo");
}

test("each format is shown and served without its metadata", async ({ page, request }, testInfo) => {
  await home(page);
  for (const file of FILES) {
    expect(identified(file.bytes).length, `${file.name} as sent holds what it names`).toBeGreaterThanOrEqual(2);
    const photo = await attachAndPost(page, file, `${file.name} in ${testInfo.project.name} at ${Date.now()}`);
    const [width, height] = file.shown;
    await expect(photo).toHaveAttribute("width", String(width));
    await expect(photo).toHaveAttribute("height", String(height));
    // Decoded, at the size its markup states.
    await expect
      .poll(() => photo.evaluate((img) => (img.complete ? [img.naturalWidth, img.naturalHeight] : null)))
      .toEqual([width, height]);
    const src = await photo.getAttribute("src");
    const served = await request.get(src);
    expect(served.status()).toBe(200);
    const bytes = Buffer.from(await served.body());
    expect(identified(bytes), `${file.name} as served`).toEqual([]);
    // The bytes kept are the bytes served, at their own key.
    const key = createHash("sha256").update(bytes).digest("hex");
    expect(src).toBe(`/images/${key}.${file.kind}`);
  }
});

test("a phone's portrait photo keeps its turn", async ({ page }, testInfo) => {
  await home(page);
  const photo = await attachAndPost(page, FILES[0], `Upright in ${testInfo.project.name} at ${Date.now()}`);
  await expect
    .poll(() => photo.evaluate((img) => (img.complete ? [img.naturalWidth, img.naturalHeight] : null)))
    .toEqual([5, 8]);
  // Turned a quarter clockwise: the stored bottom-left pixel (no red, full
  // green) is shown at the top left, the stored top-left (neither) at the
  // top right.
  const corners = await photo.evaluate((img) => {
    const canvas = document.createElement("canvas");
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const g = canvas.getContext("2d");
    g.drawImage(img, 0, 0);
    const at = (x, y) => Array.from(g.getImageData(x, y, 1, 1).data.slice(0, 2));
    return { topLeft: at(0, 0), topRight: at(canvas.width - 1, 0) };
  });
  expect(corners.topLeft[0], "red, top left").toBeLessThan(96);
  expect(corners.topLeft[1], "green, top left").toBeGreaterThan(160);
  expect(corners.topRight[1], "green, top right").toBeLessThan(96);
});

test("what each engine shows of a file as it was sent", async ({ page }, testInfo) => {
  await page.goto("about:blank");
  const sent = [
    ["a PNG whose eXIf says Orientation 6, stored 6 by 4", "image/png", editedPng()],
    ["a WebP whose EXIF says Orientation 6, stored 12 by 9", "image/webp", editedWebp()],
    ["a JPEG whose Exif says 6, then a second Exif 1, stored 8 by 5", "image/jpeg", twiceExifJpeg()],
  ];
  for (const [what, mime, bytes] of sent) {
    const size = await page.evaluate(async (src) => {
      const img = new Image();
      img.src = src;
      await img.decode();
      return [img.naturalWidth, img.naturalHeight];
    }, `data:${mime};base64,${bytes.toString("base64")}`);
    expect(size[0]).toBeGreaterThan(0);
    // A line the evidence keeps (scripts/run_suite.sh keeps `EVIDENCE` lines).
    console.log(`EVIDENCE as sent (${testInfo.project.name}): ${what}: shown ${size[0]} by ${size[1]}`);
  }
});
