// Track `upload-metadata` (ADR-0383, amending ADR-0260): an uploaded image
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
//     WebP by its EXIF, which of a JPEG's two Exif structures it reads, and
//     whether it reads an Exif after an XMP APP1.
// The independent review (2026-10-25) added:
//   - a display's ICC profile in a JPEG (two APP2 chunks), a PNG (an iCCP
//     named for a person) and a WebP (ICCP), naming a person, a device, its
//     serial number and a time: served without any of it (a PNG's profile
//     inflated to be searched), and decoded to the same colour as the file
//     as sent (EVIDENCE lines record whether each engine renders by the
//     profile at all);
//   - a JPEG whose first APP1 is XMP, then an Exif turned a quarter: served
//     turned in all three engines;
//   - a JPEG with restart markers and a GIF whose image has its own colour
//     table, built here from bytes: kept and served byte for byte, and
//     shown with their colours.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { crc32, deflateSync, inflateSync } from "node:zlib";
import { UPLOADS_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${UPLOADS_PORTS[testInfo.project.name]}`);
  },
});

// One reader's lease at a time on a host: its tests in order.
test.describe.configure({ mode: "default" });

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
/** An ICC profile's creation time, 2026-10-25 09:41:00 (six big-endian 16-bit numbers). */
const CREATED = Buffer.from([0x07, 0xea, 0, 10, 0, 25, 0, 9, 0, 41, 0, 0]);
/** An ICC profile's ID, which is an MD5 of the profile as it was. */
const PROFILE_ID = latin1("Pleris-profileID");
const SERIAL = latin1("SN 4815162342");
const utf16 = (text) => Buffer.from([...text].flatMap((c) => [0, c]));
/** Each known value, a text's as ASCII and as UTF-16BE (as an ICC profile's `mluc` holds it). */
const KNOWN = {
  "producer CMM": [latin1("CMM!")],
  "producer platform": [latin1("OS!!")],
  "producer creator": [latin1("APP!")],
  "a GPS latitude": [LATITUDE],
  "an XMP latitude": [XMP_LATITUDE],
  "a camera": [CAMERA, utf16(CAMERA)],
  "when it was taken": [TAKEN],
  "a person's name": [NAME, utf16(NAME)],
  "a comment": [COMMENT, utf16(COMMENT)],
  "an ICC profile's creation time": [CREATED],
  "an ICC profile's ID": [PROFILE_ID],
  "a serial number": [SERIAL, utf16(SERIAL)],
};
/** Which known values `bytes` holds, a PNG's ICC profiles inflated and searched too. */
const identified = (bytes) => {
  const searched = Buffer.concat([bytes, ...pngProfiles(bytes)]);
  return Object.keys(KNOWN).filter((what) => KNOWN[what].some((value) => searched.includes(value)));
};

/** A PNG's chunks before its IEND, each `[type, data]` (none for another kind). */
function* pngChunks(bytes) {
  if (bytes.subarray(1, 4).toString("latin1") !== "PNG") return;
  for (let at = 8; at + 8 <= bytes.length; at += 12 + bytes.readUInt32BE(at)) {
    const type = bytes.subarray(at + 4, at + 8).toString("latin1");
    if (type === "IEND") return;
    yield [type, bytes.subarray(at + 8, at + 8 + bytes.readUInt32BE(at))];
  }
}

/** A PNG's iCCP profiles, inflated (none for another kind). */
function pngProfiles(bytes) {
  return [...pngChunks(bytes)]
    .filter(([type]) => type === "iCCP")
    .map(([, data]) => inflateSync(data.subarray(data.indexOf(0) + 2)));
}

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

// ---- A display's ICC profile, as a screenshot embeds it (ICC.1; the
// server's `uploads/icc.rs` tests build the same). ----

/** An s15Fixed16Number. */
const s15 = (v) => {
  const b = Buffer.alloc(4);
  b.writeInt32BE(Math.round(v * 65536));
  return b;
};
const xyz = (x, y, z) => Buffer.concat([latin1("XYZ \0\0\0\0"), s15(x), s15(y), s15(z)]);

/** A text type holding `text`: version 4's `mluc` (English, UTF-16BE), or version 2's `desc` or `text`. */
function textOf(version, type, text) {
  if (version >= 4) {
    return Buffer.concat([latin1("mluc\0\0\0\0"), u32(1), u32(12), latin1("enUS"), u32(2 * text.length), u32(28), utf16(text)]);
  }
  if (type === "desc") {
    return Buffer.concat([latin1("desc\0\0\0\0"), u32(text.length + 1), text, Buffer.alloc(1 + 4 + 4 + 2 + 1 + 67)]);
  }
  return Buffer.concat([latin1("text\0\0\0\0"), text, Buffer.alloc(1)]);
}

/** A display profile's header, version 2 or 4: its creation time, its device's maker and model, its creator and ID. */
function profileHeader(version) {
  const header = Buffer.alloc(128);
  header.write("CMM!", 4, "latin1");
  header[8] = version;
  header[9] = version >= 4 ? 0x30 : 0x10;
  header.write("mntrRGB XYZ ", 12, "latin1");
  CREATED.copy(header, 24);
  header.write("acspOS!!", 36, "latin1");
  Buffer.from([...latin1("PLRSP7PR"), 0xde, 0xad, 0xbe, 0xef]).copy(header, 48);
  Buffer.concat([s15(0.9642), s15(1), s15(0.8249)]).copy(header, 68);
  header.write("APP!", 80, "latin1");
  PROFILE_ID.copy(header, 84);
  return header;
}

/**
 * A display's profile, version 2 or 4: what names a person (its description), a device (its maker's and model's
 * descriptions, Apple's `mmod`) and its serial number, and a time (its header's creation time, a calibration's `calt`),
 * its header's ID; and its colour: a D50 white, sRGB's primaries adapted to D50, and one curve its channels share,
 * linear, so an engine that renders by the profile shows a mid grey lighter.
 */
function displayProfile(version) {
  const curve =
    version >= 4 ? Buffer.concat([latin1("para\0\0\0\0\0\0\0\0"), s15(1)]) : latin1("curv\0\0\0\0\0\0\0\x01\x01\x00");
  const tags = [
    [["desc"], textOf(version, "desc", NAME)],
    [["dmnd"], textOf(version, "desc", CAMERA)],
    [["dmdd"], textOf(version, "desc", Buffer.concat([CAMERA, latin1(" "), SERIAL]))],
    [["cprt"], textOf(version, "text", COMMENT)],
    [["mmod"], Buffer.concat([latin1("mmod\0\0\0\0PLRSP7PR"), SERIAL, Buffer.alloc(3)])],
    [["calt"], Buffer.concat([latin1("dtim\0\0\0\0"), CREATED])],
    [["wtpt"], xyz(0.9642, 1, 0.8249)],
    [["rXYZ"], xyz(0.4361, 0.2225, 0.0139)],
    [["gXYZ"], xyz(0.3851, 0.7169, 0.0971)],
    [["bXYZ"], xyz(0.1431, 0.0606, 0.7141)],
    [["rTRC", "gTRC", "bTRC"], curve],
    [["chad"], Buffer.concat([latin1("sf32\0\0\0\0"), ...[1, 0, 0, 0, 1, 0, 0, 0, 1].map(s15)])],
  ];
  const count = tags.reduce((n, [sigs]) => n + sigs.length, 0);
  const header = profileHeader(version);
  const table = Buffer.alloc(4 + 12 * count);
  table.writeUInt32BE(count, 0);
  const pad4 = (n) => (n + 3) & ~3;
  const data = [];
  let at = 128 + table.length;
  let entry = 4;
  for (const [sigs, datum] of tags) {
    const start = pad4(at);
    data.push(Buffer.alloc(start - at), datum);
    for (const sig of sigs) {
      table.write(sig, entry, "latin1");
      table.writeUInt32BE(start, entry + 4);
      table.writeUInt32BE(datum.length, entry + 8);
      entry += 12;
    }
    at = start + datum.length;
  }
  const profile = Buffer.concat([header, table, ...data, Buffer.alloc(pad4(at) - at)]);
  profile.writeUInt32BE(profile.length, 0);
  return profile;
}

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

/** eight-by-five.jpeg with a display's profile (version 2) in two APP2 chunks. */
function profiledJpeg(profile = displayProfile(2)) {
  const base = fixture("eight-by-five.jpeg");
  const half = profile.length >> 1;
  const chunk = (n, part) => segment(0xe2, Buffer.concat([latin1("ICC_PROFILE\0"), Buffer.from([n, 2]), part]));
  return Buffer.concat([base.subarray(0, 20), chunk(1, profile.subarray(0, half)), chunk(2, profile.subarray(half)), base.subarray(20)]);
}

/** six-by-four.png with a display's profile (version 4) in an iCCP named for a person. */
function profiledPng(profile = displayProfile(4)) {
  const base = fixture("six-by-four.png");
  const iccp = pngChunk("iCCP", Buffer.concat([NAME, Buffer.from([0, 0]), deflateSync(profile)]));
  return Buffer.concat([base.subarray(0, 33), iccp, base.subarray(33)]);
}

/** twelve-by-nine-alpha.webp with a display's profile (version 4) in an ICCP, flagged in its VP8X. */
function profiledWebp(profile = displayProfile(4)) {
  const base = Buffer.from(fixture("twelve-by-nine-alpha.webp"));
  base[20] |= 0x20;
  return riff([base.subarray(12, 30), riffChunk("ICCP", profile), base.subarray(30)]);
}

/** eight-by-five.jpeg whose first APP1 is XMP, then an Exif turned a quarter (6). */
function xmpFirstJpeg() {
  const base = fixture("eight-by-five.jpeg");
  const xmpSegment = segment(0xe1, Buffer.concat([latin1("http://ns.adobe.com/xap/1.0/\0"), XMP]));
  return Buffer.concat([base.subarray(0, 2), xmpSegment, exifSegment(6), base.subarray(2)]);
}

/**
 * A JPEG with restart markers, as a phone's or a camera's hardware encoder writes them (the server's
 * `restarted_jpeg` builds the same): 16 by 8, three components each sampled 1 by 1, a restart after each MCU; each
 * block its DC alone, so the left MCU is grey 64 and the right, after the restart has reset its prediction, grey 192.
 */
function restartedJpeg() {
  const dc = Buffer.from([0x00, 1, 1, ...Array(14).fill(0), 0, 7]);
  const ac = Buffer.from([0x10, 1, ...Array(15).fill(0), 0x00]);
  return Buffer.concat([
    Buffer.from([0xff, 0xd8]),
    segment(0xdb, Buffer.from([0x00, 8, ...Array(63).fill(1)])),
    segment(0xc0, Buffer.from([8, 0, 8, 0, 16, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0])),
    segment(0xc4, Buffer.concat([dc, ac])),
    segment(0xdd, Buffer.from([0, 1])),
    segment(0xda, Buffer.from([3, 1, 0x00, 2, 0x00, 3, 0x00, 0, 63, 0])),
    Buffer.from([0x9f, 0x83, 0xff, 0xd0, 0xa0, 0x03, 0xff, 0xd9]),
  ]);
}

/**
 * A GIF whose image has its own colour table (the server's `local_table_gif` builds the same): a 4 by 2 screen whose
 * global table is black and white, and an image whose own table is red, blue, green and white, its left half red and
 * its right half blue (LZW codes of three bits, a clear code before every two pixels).
 */
function localTableGif() {
  const codes = [4, 0, 0, 4, 1, 1, 4, 0, 0, 4, 1, 1, 5];
  const packed = [];
  let [bits, held] = [0, 0];
  for (const code of codes) {
    bits |= code << held;
    held += 3;
    for (; held >= 8; held -= 8, bits >>= 8) packed.push(bits & 0xff);
  }
  if (held > 0) packed.push(bits & 0xff);
  return Buffer.concat([
    latin1("GIF89a"),
    Buffer.from([4, 0, 2, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255]),
    Buffer.from([0x2c, 0, 0, 0, 0, 4, 0, 2, 0, 0x81, 255, 0, 0, 0, 0, 255, 0, 255, 0, 255, 255, 255, 2]),
    subBlocks(Buffer.from(packed)),
    Buffer.from([0x3b]),
  ]);
}

const FILES = [
  { name: "IMG_0001.jpg", kind: "jpeg", mime: "image/jpeg", bytes: phoneJpeg(), shown: [5, 8] },
  { name: "edited.png", kind: "png", mime: "image/png", bytes: editedPng(), shown: [6, 4] },
  { name: "edited.webp", kind: "webp", mime: "image/webp", bytes: editedWebp(), shown: [12, 9] },
  { name: "animated.webp", kind: "webp", mime: "image/webp", bytes: animatedWebp(), shown: [7, 3] },
  { name: "edited.gif", kind: "gif", mime: "image/gif", bytes: editedGif(), shown: [5, 9] },
  { name: "xmp-first.jpg", kind: "jpeg", mime: "image/jpeg", bytes: xmpFirstJpeg(), shown: [5, 8] },
  { name: "profiled.jpg", kind: "jpeg", mime: "image/jpeg", bytes: profiledJpeg(), shown: [8, 5], bare: "eight-by-five.jpeg" },
  { name: "profiled.png", kind: "png", mime: "image/png", bytes: profiledPng(), shown: [6, 4], bare: "six-by-four.png" },
  { name: "profiled.webp", kind: "webp", mime: "image/webp", bytes: profiledWebp(), shown: [12, 9], bare: "twelve-by-nine-alpha.webp" },
];

/** What a blank page decodes `bytes` to: the colour of its pixel at 1, 1 (as a canvas reads it, in sRGB). */
async function colourOf(page, mime, bytes) {
  return page.evaluate(async (src) => {
    const img = new Image();
    img.src = src;
    await img.decode();
    const canvas = document.createElement("canvas");
    [canvas.width, canvas.height] = [img.naturalWidth, img.naturalHeight];
    const g = canvas.getContext("2d");
    g.drawImage(img, 0, 0);
    return Array.from(g.getImageData(1, 1, 1, 1).data);
  }, `data:${mime};base64,${bytes.toString("base64")}`);
}

// ---- The feed. ----

/** The home page, its handlers attached. */
async function home(page) {
  await page.goto("/");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/**
 * A profile kept for its colour: each file as served (`profiled`, `[file, bytes served]`) decodes to the colour of the
 * file as sent. The feed's page allows no `data:` image, so both are decoded on a new page's blank one (WebKit keeps
 * the feed's policy in the `about:blank` the feed's own page goes to, and Chromium and Firefox do not); the fixture
 * without a profile says whether this engine renders by a profile at all.
 */
async function expectColoursAsSent(page, profiled, project) {
  const blank = await page.context().newPage();
  await blank.goto("about:blank");
  for (const [file, served] of profiled) {
    const sent = await colourOf(blank, file.mime, file.bytes);
    const bare = await colourOf(blank, file.mime, fixture(file.bare));
    expect(await colourOf(blank, file.mime, served), `${file.name}: its colour as sent`).toEqual(sent);
    const by = sent.join() === bare.join() ? "does not render by its profile" : "renders by its profile";
    console.log(
      `EVIDENCE colour (${project}): ${file.name} ${by}: as sent and as served [${sent}], without a profile [${bare}]`,
    );
  }
  await blank.close();
}

/** Attach `file` with the home page's form, and post it with `text`. */
async function attachForPreview(page, file) {
  await page.locator("#attach input[type=file]").setInputFiles({ name: file.name, mimeType: file.mime, buffer: file.bytes });
  await Promise.all([
    page.waitForNavigation(),
    page.locator("#attach").getByRole("button", { name: "Attach", exact: true }).click(),
  ]);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

async function attachAndPost(page, file, text) {
  await attachForPreview(page, file);
  await page.getByLabel("What's happening?").fill(text);
  await page.getByLabel("The image, in words, for whoever cannot see it").fill(`${file.name}, in words`);
  await page.getByRole("button", { name: "Post", exact: true }).click();
  return page.locator("main > ul > li").filter({ hasText: text }).locator("img.photo");
}

test("each format is shown and served without its metadata", async ({ page, request }, testInfo) => {
  await home(page);
  const profiled = [];
  for (const file of FILES) {
    expect(identified(file.bytes).length, `${file.name} as sent holds what it names`).toBeGreaterThanOrEqual(2);
    const photo = await attachAndPost(page, file, `${file.name} in ${testInfo.project.name} at ${Date.now()}`);
    const [width, height] = file.shown;
    await expect(photo).toHaveAttribute("width", String(width));
    await expect(photo).toHaveAttribute("height", String(height));
    // Decoded, at the size its markup states.
    await expect
      .poll(() => photo.evaluate((img) => (img.complete ? `${img.naturalWidth}x${img.naturalHeight}` : null)))
      .toBe(`${width}x${height}`);
    const src = await photo.getAttribute("src");
    const served = await request.get(src);
    expect(served.status()).toBe(200);
    const bytes = Buffer.from(await served.body());
    expect(identified(bytes), `${file.name} as served`).toEqual([]);
    // The bytes kept are the bytes served, at their own key.
    const key = createHash("sha256").update(bytes).digest("hex");
    expect(src).toBe(`/images/${key}.${file.kind}`);
    if (file.bare) profiled.push([file, bytes]);
  }
  await expectColoursAsSent(page, profiled, testInfo.project.name);
});

test("a JPEG with restart markers and a GIF with its own palette are kept whole and shown", async ({ page, request }, testInfo) => {
  await home(page);
  const whole = [
    {
      name: "restarted.jpg",
      kind: "jpeg",
      mime: "image/jpeg",
      bytes: restartedJpeg(),
      shown: [16, 8],
      colours: [[[3, 4], [64, 64, 64]], [[12, 4], [192, 192, 192]]],
    },
    {
      name: "palettes.gif",
      kind: "gif",
      mime: "image/gif",
      bytes: localTableGif(),
      shown: [4, 2],
      colours: [[[0, 0], [255, 0, 0]], [[3, 1], [0, 0, 255]]],
    },
  ];
  for (const file of whole) {
    const photo = await attachAndPost(page, file, `${file.name} in ${testInfo.project.name} at ${Date.now()}`);
    const [width, height] = file.shown;
    await expect(photo).toHaveAttribute("width", String(width));
    await expect(photo).toHaveAttribute("height", String(height));
    await expect
      .poll(() => photo.evaluate((img) => (img.complete ? `${img.naturalWidth}x${img.naturalHeight}` : null)))
      .toBe(`${width}x${height}`);
    // Nothing to drop, so kept and served byte for byte.
    const src = await photo.getAttribute("src");
    const served = Buffer.from(await (await request.get(src)).body());
    expect(served.equals(file.bytes), `${file.name} served as sent`).toBe(true);
    expect(src).toBe(`/images/${createHash("sha256").update(file.bytes).digest("hex")}.${file.kind}`);
    const read = await photo.evaluate((img, points) => {
      const canvas = document.createElement("canvas");
      [canvas.width, canvas.height] = [img.naturalWidth, img.naturalHeight];
      const g = canvas.getContext("2d");
      g.drawImage(img, 0, 0);
      return points.map(([x, y]) => Array.from(g.getImageData(x, y, 1, 1).data.slice(0, 3)));
    }, file.colours.map(([at]) => at));
    file.colours.forEach(([at, colour], n) => {
      read[n].forEach((value, channel) => {
        expect(Math.abs(value - colour[channel]), `${file.name} at ${at}: ${read[n]}`).toBeLessThanOrEqual(2);
      });
    });
  }
});

test("a phone's portrait photo keeps its turn", async ({ page }, testInfo) => {
  await home(page);
  const photo = await attachAndPost(page, FILES[0], `Upright in ${testInfo.project.name} at ${Date.now()}`);
  await expect
    .poll(() => photo.evaluate((img) => (img.complete ? `${img.naturalWidth}x${img.naturalHeight}` : null)))
    .toBe("5x8");
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
    ["a JPEG whose first APP1 is XMP, then an Exif that says 6, stored 8 by 5", "image/jpeg", xmpFirstJpeg()],
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


for (const orientation of [2, 3, 4, 5, 6, 7, 8]) {
  test(`JPEG orientation ${orientation} keeps the same displayed pixels`, async ({ page, request }, testInfo) => {
    const base = fixture("eight-by-five.jpeg");
    const bytes = Buffer.concat([base.subarray(0, 2), exifSegment(orientation), base.subarray(2)]);
    const file = { name: `orientation-${orientation}.jpg`, mime: "image/jpeg", bytes };
    await home(page);
    const photo = await attachAndPost(page, file, `${file.name} ${testInfo.project.name} ${Date.now()}`);
    const served = Buffer.from(await (await request.get(await photo.getAttribute("src"))).body());
    const blank = await page.context().newPage();
    try {
      expect(await pixelsOf(blank, served)).toEqual(await pixelsOf(blank, bytes));
    } finally {
      await blank.close();
    }
  });
}

async function pixelsOf(page, bytes, mime = "image/jpeg") {
  return page.evaluate(async (src) => {
    const image = new Image();
    image.src = src;
    await image.decode();
    const canvas = document.createElement("canvas");
    [canvas.width, canvas.height] = [image.naturalWidth, image.naturalHeight];
    const context = canvas.getContext("2d");
    context.drawImage(image, 0, 0);
    return [canvas.width, canvas.height, ...context.getImageData(0, 0, canvas.width, canvas.height).data];
  }, `data:${mime};base64,${bytes.toString("base64")}`);
}

test("the real ICC sRGB profile stays decodable with the same colour", async ({ page, request }, testInfo) => {
  const profile = fixture("sRGB2014.icc");
  const files = [
    { name: "real.jpg", mime: "image/jpeg", bytes: profiledJpeg(profile), bare: "eight-by-five.jpeg" },
    { name: "real.png", mime: "image/png", bytes: profiledPng(profile), bare: "six-by-four.png" },
    { name: "real.webp", mime: "image/webp", bytes: profiledWebp(profile), bare: "twelve-by-nine-alpha.webp" },
  ];
  await home(page);
  const kept = [];
  for (const file of files) {
    const photo = await attachAndPost(page, file, `${file.name} ${testInfo.project.name} ${Date.now()}`);
    const answer = await request.get(await photo.getAttribute("src"));
    expect(answer.status()).toBe(200);
    kept.push([file, Buffer.from(await answer.body())]);
  }
  await expectColoursAsSent(page, kept, testInfo.project.name);
});


test("PNG carrier refusals never publish an original blob", async ({ page, request }) => {
  await home(page);
  const origin = new URL(page.url()).origin;
  for (const name of ["late-chrm", "duplicate-chrm", "idat-suffix", "later-idat-suffix", "decoded-suffix", "apng-suffix", "orphan-fdat", "bad-adler", "apng-interleaved", "research-late-chrm", "research-duplicate-chrm", "research-idat-suffix"]) {
    const bytes = fixture(`privacy-${name}.png`);
    const answer = await page.request.post("/uploads/post-image", {
      multipart: { image: { name: `${name}.png`, mimeType: "image/png", buffer: bytes } },
      headers: { Origin: origin, Referer: `${origin}/`, "Sec-Fetch-Site": "same-origin" },
      maxRedirects: 0,
    });
    expect(answer.status(), "a tolerated carrier is refused before staging").toBe(422);
    const original = `/images/${createHash("sha256").update(bytes).digest("hex")}.png`;
    expect((await request.get(original)).status()).toBe(404);
  }
});

async function postPreview(page, text) {
  await page.getByLabel("What's happening?").fill(text);
  await page.getByLabel("The image, in words, for whoever cannot see it").fill("A synthetic test image");
  await page.getByRole("button", { name: "Post", exact: true }).click();
  const image = page.locator("main > ul > li").filter({ hasText: text }).locator("img.photo");
  await expect(image).toHaveCount(1);
  return image;
}

async function previewRoundtrip(page, request, name, label) {
  const bytes = fixture(`privacy-${name}.png`);
  await attachForPreview(page, { name: `${name}.png`, mime: "image/png", bytes });
  const staged = page.locator('img[src^="/uploads/post-image/"]');
  await expect(staged).toHaveCount(1);
  const previewURL = await staged.getAttribute("src");
  const previewAnswer = await page.request.get(previewURL);
  expect(previewAnswer.status()).toBe(200);
  const preview = Buffer.from(await previewAnswer.body());
  const image = await postPreview(page, label);
  const src = await image.getAttribute("src");
  const served = Buffer.from(await (await request.get(src)).body());
  expect(served.equals(preview), "the committed body is the rewritten preview").toBe(true);
  expect(src).toBe(`/images/${createHash("sha256").update(preview).digest("hex")}.png`);
  expect((await page.request.get(previewURL)).status()).toBe(404);
  return { bytes, served };
}

test("PNG stream and ICC repairs survive preview claim storage and serving", async ({ page, request }, testInfo) => {
  await home(page);
  const blank = await page.context().newPage();
  try {
    for (const name of ["duplicate-icc", "valid-icc", "split-idat", "apng-included", "apng-excluded", "apng-split"]) {
      const { bytes, served } = await previewRoundtrip(page, request, name, `${name} ${testInfo.project.name} ${Date.now()}`);
      const reference = name === "duplicate-icc" ? fixture("privacy-clean.png") : bytes;
      expect(await pixelsOf(blank, served, "image/png"), name).toEqual(await pixelsOf(blank, reference, "image/png"));
      if (name.endsWith("duplicate-icc")) expect(pngProfiles(served)).toEqual([]);
    }
  } finally {
    await blank.close();
  }
});

test("PNG packed sixteen bit and tiny Adam7 neighbors keep all displayed pixels", async ({ page, request }, testInfo) => {
  test.setTimeout(90_000); // Eleven full upload/post/decode neighbors, including WebKit.
  await home(page);
  const blank = await page.context().newPage();
  try {
    for (const name of ["layout-0-1", "layout-0-8", "layout-0-16", "layout-2-8", "layout-2-16", "layout-4-16", "layout-6-16", "layout-3-1", "layout-3-2", "layout-3-4", "adam7-tiny"]) {
      const bytes = fixture(`privacy-${name}.png`);
      const image = await attachAndPost(page, { name: `${name}.png`, mime: "image/png", bytes }, `${name} ${testInfo.project.name} ${Date.now()}`);
      const served = Buffer.from(await (await request.get(await image.getAttribute("src"))).body());
      expect(await pixelsOf(blank, served, "image/png")).toEqual(await pixelsOf(blank, bytes, "image/png"));
    }
  } finally {
    await blank.close();
  }
});
