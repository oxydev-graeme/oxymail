// Sidebar backgrounds (Settings → Appearance): a ready-made design, the user's own colour, or
// the user's own picture. Every background comes in a light and a dark version, and the one
// shown follows the theme, so a light picture is never put behind light text.
//
// Readability is kept by construction, not by a scrim: every colour and every pixel is kept
// inside a band of relative luminance in which the sidebar's muted text (the palest text on it)
// still has a WCAG AA contrast of 4.5:1.
//   light: --muted #5f6b7a has Y 0.1435, so the background needs Y ≥ 0.821
//   dark:  --muted #96a3b0 has Y 0.3581, so the background needs Y ≤ 0.041
// The bands below keep room for JPEG rounding and for gradients blending in sRGB
// (`npm run shots` measures a deliberately harsh picture and prints the range it got).
const BAND = { light: [0.86, 1], dark: [0.004, 0.03] };

// ---- Colour maths (OKLCH ↔ linear sRGB) --------------------------------------------------
const toLinear = (v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const toGamma = (v) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);
const luminance = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;

function oklchToLinear(L, C, h) {
  const a = C * Math.cos((h * Math.PI) / 180), b = C * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map((v) => Math.min(1, Math.max(0, v)));
}

function hexToOklch(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => toLinear(parseInt(hex.slice(i, i + 2), 16) / 255));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return { L, C: Math.hypot(a, B), h: ((Math.atan2(B, a) * 180) / Math.PI + 360) % 360 };
}

const hex = (lin) => "#" + lin.map((v) => Math.round(toGamma(v) * 255).toString(16).padStart(2, "0")).join("");

/** A colour of hue `h` near lightness `L`, moved just far enough to sit inside the theme's band. */
function tone(L, C, h, dark) {
  const [lo, hi] = BAND[dark ? "dark" : "light"];
  for (let i = 0; i < 80; i++) {
    const y = luminance(oklchToLinear(L, C, h));
    if (y > hi) L -= 0.005;
    else if (y < lo) L += 0.005;
    else break;
  }
  return hex(oklchToLinear(L, C, h));
}

// ---- Ready-made designs ------------------------------------------------------------------
// Soft washes of two hues; [name, first hue, second hue, how colourful].
export const PRESETS = [
  ["lagoon", "Lagoon", 215, 190, 1],
  ["dawn", "Dawn", 25, 75, 1],
  ["meadow", "Meadow", 145, 105, 0.9],
  ["lavender", "Lavender", 295, 255, 1],
  ["sand", "Sand", 75, 50, 0.7],
  ["slate", "Slate", 250, 230, 0.45],
].map(([id, name, h1, h2, k]) => ({ id, name, h1, h2, k }));

function presetCss({ h1, h2, k }, dark) {
  const [baseL, spotL, baseC, spotC] = dark ? [0.2, 0.3, 0.02, 0.065] : [0.975, 0.945, 0.012, 0.05];
  const base = tone(baseL, baseC * k, h1, dark);
  const a = tone(spotL, spotC * k, h1, dark);
  const b = tone(spotL, spotC * k, h2, dark);
  return `radial-gradient(120% 60% at 0% 0%, ${a}, transparent 70%), radial-gradient(120% 70% at 100% 100%, ${b}, transparent 70%), ${base}`;
}

/** The user's colour keeps its hue and some of its colourfulness; its lightness follows the theme. */
function colorCss(value, dark) {
  const { C, h } = hexToOklch(value);
  return dark ? tone(0.27, Math.min(C, 0.07), h, true) : tone(0.95, Math.min(C, 0.05), h, false);
}

/**
 * The CSS `background` for the sidebar, or "" for the theme's own colour.
 * sidebar_bg is "", a preset id, "color" (sidebar_color) or "image" (sidebar_image_light/dark).
 */
export function sidebarBackground(settings, dark) {
  const kind = settings.sidebar_bg || "";
  if (kind === "color" && /^#[0-9a-f]{6}$/i.test(settings.sidebar_color ?? "")) return colorCss(settings.sidebar_color, dark);
  if (kind === "image") {
    const src = settings[dark ? "sidebar_image_dark" : "sidebar_image_light"];
    return src ? `url("${src}") center / cover no-repeat` : "";
  }
  const preset = PRESETS.find((p) => p.id === kind);
  return preset ? presetCss(preset, dark) : "";
}

// ---- The user's picture ------------------------------------------------------------------
/** Longest side kept: the sidebar is at most 420px wide, so this stays sharp at 200% scaling. */
const MAX_SIDE = 900;

/**
 * Reads a picture and makes its light and dark versions as JPEG data URLs. Each pixel keeps
 * its colour but its luminance is squeezed into the theme's band (a light version is a pale
 * wash of the picture, a dark one a deep shadow of it), so text on top always stays readable.
 */
export async function tonePicture(file) {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const w = Math.max(1, Math.round(bitmap.width * scale)), h = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(bitmap, 0, 0, w, h);
  bitmap.close?.();
  const source = ctx.getImageData(0, 0, w, h);

  // Softened a little first: a background shouldn't compete with the text, and hard edges are
  // where JPEG overshoots the band.
  const lut = new Float32Array(256).map((_, i) => toLinear(i / 255));
  const lin = new Float32Array(w * h * 3);
  for (let i = 0, j = 0; i < source.data.length; i += 4, j += 3) {
    lin[j] = lut[source.data[i]]; lin[j + 1] = lut[source.data[i + 1]]; lin[j + 2] = lut[source.data[i + 2]];
  }
  boxBlur(lin, w, h, 2);
  const version = (dark) => {
    const [lo, hi] = BAND[dark ? "dark" : "light"];
    const out = new ImageData(w, h);
    const d = out.data;
    for (let i = 0, j = 0; i < d.length; i += 4, j += 3) {
      let r = lin[j], g = lin[j + 1], b = lin[j + 2];
      const y = luminance([r, g, b]);
      const target = lo + (hi - lo) * y;
      if (y > 1e-5) {
        const k = target / y;
        r *= k; g *= k; b *= k;
      } else r = g = b = target;
      // Too bright a channel to scale (a vivid colour made pale): blend toward grey of the same luminance.
      const top = Math.max(r, g, b);
      if (top > 1) {
        const t = (1 - target) / (top - target);
        r = target + (r - target) * t; g = target + (g - target) * t; b = target + (b - target) * t;
      }
      // A pale wash stays pastel: JPEG stores colour at half resolution, which darkens vivid
      // pale colours (a bright yellow) next to other colours, out of the readable band.
      const floor = dark ? 0 : target * 0.72, bottom = Math.min(r, g, b);
      if (bottom < floor) {
        const t = (target - floor) / (target - bottom);
        r = target + (r - target) * t; g = target + (g - target) * t; b = target + (b - target) * t;
      }
      d[i] = Math.round(toGamma(r) * 255);
      d[i + 1] = Math.round(toGamma(g) * 255);
      d[i + 2] = Math.round(toGamma(b) * 255);
      d[i + 3] = 255;
    }
    ctx.putImageData(out, 0, 0);
    return canvas.toDataURL("image/jpeg", 0.9);
  };
  return { light: version(false), dark: version(true) };
}

/** Blurs interleaved RGB in place with a box of the given radius, across then down. */
function boxBlur(px, w, h, radius) {
  const pass = (count, length, at) => {
    const line = new Float32Array(length * 3);
    for (let n = 0; n < count; n++) {
      for (let k = 0; k < length; k++) for (let c = 0; c < 3; c++) line[k * 3 + c] = px[at(n, k) + c];
      for (let k = 0; k < length; k++) {
        const a = Math.max(0, k - radius), b = Math.min(length - 1, k + radius);
        for (let c = 0; c < 3; c++) {
          let sum = 0;
          for (let q = a; q <= b; q++) sum += line[q * 3 + c];
          px[at(n, k) + c] = sum / (b - a + 1);
        }
      }
    }
  };
  pass(h, w, (y, x) => (y * w + x) * 3);
  pass(w, h, (x, y) => (y * w + x) * 3);
}
