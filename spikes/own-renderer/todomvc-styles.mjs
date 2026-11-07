// TodoMVC's two drawings of a todo's checkbox, drawn by this app's own
// stylesheet (track `todomvc`, W7; run by todomvc.sh).
//
// The official stylesheet (`todomvc-app-css`, CC BY 4.0, Sindre Sorhus) draws
// a todo's checkbox, a circle and a ticked circle, as `data:` images in two
// rules. A page's generated policy allows images of its own origin only
// (`img-src 'self'`, ADR-0333), so the sheet is shown without them.
//
// Until 2026-11-06 this wrote each drawing out as an SVG file, and the rules
// pointed at the files. A page then fetched the circle with its first todo
// and the tick with its first toggle: one request each, and each drawing
// shown a round trip after the frame that showed its todo (562.5 ms under
// Lighthouse's Slow 4G). Now this reads the two drawings out of the sheet,
// as it ships, and writes them in CSS, in this app's own sheet, loaded after
// the official one as TodoMVC's spec allows ("if you need to change some
// styles, use the app.css file"): each circle a radial gradient, and the
// tick a polygon, at the places the drawing's own viewBox puts them. Nothing
// here is drawn by hand, so the drawings are the official ones;
// `e2e/todomvc.spec.mjs` holds each to the official drawing, pixel by pixel.
// A sheet whose drawings this cannot read fails this script, rather than
// shipping a page without its checkboxes.
//
// usage: node todomvc-styles.mjs <todomvc-app-css/index.css> <out directory>

import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

/** The drawing the rule headed `selector` gives its background, as SVG. */
export function drawingIn(sheet, selector) {
  const rule = sheet
    .split("}")
    .find((r) => r.replace(/\s+/g, " ").trimStart().startsWith(`${selector} {`));
  const uri = /url\('data:image\/svg\+xml;utf8,([^']*)'\)/.exec(rule ?? "");
  if (!uri) throw new Error(`the official sheet draws no checkbox for ${selector}`);
  return decodeURIComponent(uri[1]);
}

/** An attribute of one element's start tag, or nothing. */
const attribute = (tag, name) => new RegExp(`\\s${name}="([^"]*)"`).exec(tag)?.[1];

function numberOf(tag, name) {
  const value = Number(attribute(tag, name));
  if (!Number.isFinite(value)) throw new Error(`a drawing's ${name} is no number: ${tag}`);
  return value;
}

function colourOf(tag, name) {
  const colour = attribute(tag, name) ?? "";
  if (!/^#[0-9a-f]{3,8}$/i.test(colour)) throw new Error(`a drawing's ${name} is no colour: ${tag}`);
  return colour;
}

function viewBoxOf(root) {
  const box = (attribute(root, "viewBox") ?? "").split(/[\s,]+/).map(Number);
  if (box.length !== 4 || !box.every(Number.isFinite)) throw new Error(`a drawing with no viewBox: ${root}`);
  return box;
}

/** **Where a drawing's own units fall in its box**: its viewBox fitted to
 * its width and height, and centred, as SVG fits one by default
 * (`xMidYMid meet`). The scale, and where the viewBox's origin falls, in
 * pixels. */
function fitOf(svg) {
  const root = /<svg\b[^>]*>/.exec(svg)?.[0] ?? "";
  if (attribute(root, "preserveAspectRatio")) throw new Error(`a drawing fitted otherwise: ${root}`);
  const [x, y, w, h] = viewBoxOf(root);
  const width = numberOf(root, "width");
  const height = numberOf(root, "height");
  const scale = Math.min(width / w, height / h);
  return { width, height, scale, x: (width - w * scale) / 2 - x * scale, y: (height - h * scale) / 2 - y * scale };
}

/** Each circle a drawing strokes, in pixels. */
function circlesOf(svg, fit) {
  return [...svg.matchAll(/<circle\b[^>]*>/g)].map(([tag]) => {
    if (attribute(tag, "fill") !== "none") throw new Error(`a filled circle: ${tag}`);
    return {
      x: fit.x + numberOf(tag, "cx") * fit.scale,
      y: fit.y + numberOf(tag, "cy") * fit.scale,
      r: numberOf(tag, "r") * fit.scale,
      width: numberOf(tag, "stroke-width") * fit.scale,
      colour: colourOf(tag, "stroke"),
    };
  });
}

/** Each path a drawing fills, as the polygon it draws, in pixels. */
function polygonsOf(svg, fit) {
  return [...svg.matchAll(/<path\b[^>]*>/g)].map(([tag]) => ({
    colour: colourOf(tag, "fill"),
    points: pointsOf(attribute(tag, "d") ?? "").map(([x, y]) => [fit.x + x * fit.scale, fit.y + y * fit.scale]),
  }));
}

/**
 * **The points a path's commands draw**, where they draw a polygon: `M` or
 * `m`, then `L` or `l`, each pair after a command one more point, and `Z` to
 * close. A curve or an arc is refused: this reads the drawing TodoMVC ships,
 * and one it cannot read must fail the build, not draw something else.
 */
export function pointsOf(d) {
  const points = [];
  const pair = [];
  let relative = false;
  for (const token of tokensOf(d)) {
    if (/[a-z]/i.test(token)) relative = relativeOf(token);
    else pair.push(Number(token));
    if (pair.length === 2) points.push(pointAfter(points, pair.splice(0), relative));
  }
  return points;
}

/** A path's commands and numbers, in order. */
const tokensOf = (d) => d.match(/[a-z]|-?(?:\d+\.?\d*|\.\d+)/gi) ?? [];

/** Whether a command's pairs are relative to the point before. */
function relativeOf(command) {
  if (!"MLmlZz".includes(command)) throw new Error(`a path that draws no polygon: ${command}`);
  return command === command.toLowerCase();
}

function pointAfter(points, [x, y], relative) {
  const [px, py] = relative ? (points.at(-1) ?? [0, 0]) : [0, 0];
  return [px + x, py + y];
}

/** One drawing, read: its box, its circles and its polygons. */
function read(svg) {
  const fit = fitOf(svg);
  return { fit, circles: circlesOf(svg, fit), polygons: polygonsOf(svg, fit) };
}

const px = (n) => `${Number(n.toFixed(3))}px`;

/** A stroked circle as a radial gradient: its colour at its radius, fading
 * to nothing a stroke's width either side, which lays the ink a stroke of
 * that width covers. (Chromium and Firefox lay 12 to 13% less for the
 * official stroke, measured 2026-11-06; the spec's ink bounds allow it.) */
function ringOf(c) {
  return `radial-gradient(circle at ${px(c.x)} ${px(c.y)}, transparent ${px(c.r - c.width)}, ${c.colour} ${px(c.r)}, transparent ${px(c.r + c.width)})`;
}

function clipOf(polygon) {
  return `polygon(${polygon.points.map(([x, y]) => `${px(x)} ${px(y)}`).join(", ")})`;
}

/** Whether the two drawings are TodoMVC's: a circle, and a circle with one
 * tick (#81's review: a drawing with no circle fails the build). */
function shapedAsTodoMVCs(circle, checked) {
  return (
    circle.circles.length === 1 &&
    circle.polygons.length === 0 &&
    checked.circles.length === 1 &&
    checked.polygons.length === 1
  );
}

/**
 * **This app's own sheet**: each drawing's circles as its label's
 * background, sized and placed where the official sheet places the image;
 * and the ticked drawing's polygon on the label's `::after`, over that same
 * box. The `::after` is placed in the item (the label stays unpositioned, so
 * that the checkbox over it keeps its clicks) and takes no pointer itself.
 */
export function sheetOf(official) {
  const circle = read(drawingIn(official, ".todo-list li .toggle + label"));
  const checked = read(drawingIn(official, ".todo-list li .toggle:checked + label"));
  if (!shapedAsTodoMVCs(circle, checked)) {
    throw new Error("the official checkbox is no longer a circle, and a circle with one tick");
  }
  const [tick] = checked.polygons;
  return [
    "/* This app's own sheet, after TodoMVC's (todomvc-app.css, CC BY 4.0, Sindre Sorhus):",
    "   the same two drawings of a todo's checkbox, in CSS, since a page's policy refuses",
    "   a data: image. Generated by todomvc-styles.mjs from that sheet's drawings. */",
    ".todo-list li .toggle + label {",
    `\tbackground-image: ${circle.circles.map(ringOf).join(", ")};`,
    `\tbackground-size: ${px(circle.fit.width)} ${px(circle.fit.height)};`,
    "}",
    "",
    ".todo-list li .toggle:checked + label {",
    `\tbackground-image: ${checked.circles.map(ringOf).join(", ")};`,
    `\tbackground-size: ${px(checked.fit.width)} ${px(checked.fit.height)};`,
    "}",
    "",
    ".todo-list li .toggle:checked + label::after {",
    '\tcontent: "";',
    "\tposition: absolute;",
    `\ttop: calc(50% - ${px(checked.fit.height / 2)});`,
    // On the row of pixels a browser draws the background's image on: it
    // draws an image whole-pixel aligned, and lays a box out where it falls.
    `\ttop: round(nearest, calc(50% - ${px(checked.fit.height / 2)}), 1px);`,
    "\tleft: 0;",
    `\twidth: ${px(checked.fit.width)};`,
    `\theight: ${px(checked.fit.height)};`,
    `\tbackground: ${tick.colour};`,
    `\tclip-path: ${clipOf(tick)};`,
    "\tpointer-events: none;",
    "}",
    "",
  ].join("\n");
}

/** **The official checkbox's ring**, in pixels of its 40-pixel box: its
 * centre, radius, stroke width and colour. `e2e/todomvc.spec.mjs` tells the
 * ring's ink from the tick's by it. */
export function ringIn(official) {
  const [ring] = read(drawingIn(official, ".todo-list li .toggle + label")).circles;
  return ring;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [css, out] = process.argv.slice(2);
  writeFileSync(`${out}/todomvc-pleris.css`, sheetOf(readFileSync(css, "utf8")));
}
