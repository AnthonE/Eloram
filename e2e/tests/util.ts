import { PNG } from "pngjs";
import type { Page } from "@playwright/test";

/** Standard deviation of screenshot luminance: a blank or single-colour frame is ~0. */
export function luminanceStdDev(png: Buffer): number {
  const img = PNG.sync.read(png);
  let sum = 0;
  let sumSq = 0;
  const n = img.width * img.height;
  for (let i = 0; i < img.data.length; i += 4) {
    const l = 0.2126 * img.data[i] + 0.7152 * img.data[i + 1] + 0.0722 * img.data[i + 2];
    sum += l;
    sumSq += l * l;
  }
  const mean = sum / n;
  return Math.sqrt(Math.max(0, sumSq / n - mean * mean));
}

/** The median luminance (0 to 255) of the picture's middle, a box a fifth of its width and height:
 * where the camera looks. */
export function centreLuminance(png: Buffer): number {
  const img = PNG.sync.read(png);
  const [w, h] = [img.width, img.height];
  const ls: number[] = [];
  for (let y = Math.floor(h * 0.4); y < Math.floor(h * 0.6); y++) {
    for (let x = Math.floor(w * 0.4); x < Math.floor(w * 0.6); x++) {
      const i = (y * w + x) * 4;
      ls.push(0.2126 * img.data[i] + 0.7152 * img.data[i + 1] + 0.0722 * img.data[i + 2]);
    }
  }
  ls.sort((a, b) => a - b);
  return ls[Math.floor(ls.length / 2)];
}

/** Reads the client's `window.__el` status object. */
export async function el(page: Page): Promise<Record<string, any>> {
  return (await page.evaluate(() => (window as any).__el ?? {})) as Record<string, any>;
}

export function collectConsole(page: Page): string[] {
  const lines: string[] = [];
  page.on("console", (m) => lines.push(`[${m.type()}] ${m.text()}`));
  page.on("pageerror", (e) => lines.push(`[pageerror] ${e.message}`));
  return lines;
}
