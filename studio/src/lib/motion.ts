// Klipp's motion, for Svelte and script. The same numbers as the tokens in
// app.css; ui/FOUNDATION.md shows how to use each helper.
import { flushSync, tick } from "svelte";
import type { TransitionConfig } from "svelte/transition";

/** Durations (ms): --dur-fast, --dur, --dur-slow, --dur-spring. */
export const DUR = { fast: 120, base: 200, slow: 320, spring: 420 } as const;

/** Close to --ease-out, as a function for Svelte transitions. */
export const easeOut = (t: number) => 1 - (1 - t) ** 4;

/** --spring as a function: a gentle spring, about 2 % past the target. */
export function spring(t: number): number {
  if (t >= 1) return 1;
  const zeta = 0.78;
  const omega = 8.2;
  const damped = omega * Math.sqrt(1 - zeta * zeta);
  return 1 - Math.exp(-zeta * omega * t) * (Math.cos(damped * t) + ((zeta * omega) / damped) * Math.sin(damped * t));
}

/** The person asked for less motion. */
export const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * The delay (ms) for item `i` of a list that appears together. Capped, so the
 * last of a hundred arrives as soon as the ninth.
 */
export function stagger(i: number, step = 28, cap = 8): number {
  return reduced() ? 0 : Math.min(i, cap) * step;
}

interface EnterOptions {
  /** Travel in px, from below. Negative: from above. */
  y?: number;
  x?: number;
  delay?: number;
  duration?: number;
  easing?: (t: number) => number;
}

/** in:enter: fades in while travelling a little. Keep the node free of its own transform. */
export function enter(_: Element, { y = 6, x = 0, delay = 0, duration = DUR.slow, easing = easeOut }: EnterOptions = {}): TransitionConfig {
  if (reduced()) return { duration: 0 };
  return { delay, duration, easing, css: (t, u) => `opacity: ${t}; transform: translate(${u * x}px, ${u * y}px)` };
}

/** out:leave: the quick way out. */
export function leave(_: Element, { y = 0, x = 0, delay = 0, duration = DUR.fast }: EnterOptions = {}): TransitionConfig {
  if (reduced()) return { duration: 0 };
  return { delay, duration, easing: easeOut, css: (t, u) => `opacity: ${t}; transform: translate(${u * x}px, ${u * y}px)` };
}

/** in:pop / out:pop: a popover or dialog opening from where it's anchored (set transform-origin on it). */
export function pop(_: Element, { delay = 0, duration = DUR.base }: { delay?: number; duration?: number } = {}): TransitionConfig {
  if (reduced()) return { duration: 0 };
  return { delay, duration, easing: easeOut, css: (t, u) => `opacity: ${t}; transform: translateY(${u * 4}px) scale(${1 - u * 0.02})` };
}

/** in:unfold: a menu growing out of the button that opens it (set transform-origin on it). */
export function unfold(_: Element, { delay = 0, duration = DUR.spring }: { delay?: number; duration?: number } = {}): TransitionConfig {
  if (reduced()) return { duration: 0 };
  return { delay, duration, easing: spring, css: (t, u) => `opacity: ${Math.min(1, t * 2.5)}; transform: translateY(${u * -6}px) scale(${0.9 + t * 0.1})` };
}

/**
 * Page crossfade inside a view, without the View Transitions API: the old
 * content fades out in place while the new one arrives. Put the {#key} block
 * in a `.stack` so both share one spot:
 *   <div class="stack">{#key id}<div in:pageIn out:pageOut>…</div>{/key}</div>
 */
export function pageIn(_: Element, { delay = 50, y = 6 }: { delay?: number; y?: number } = {}): TransitionConfig {
  if (reduced()) return { duration: 0 };
  return { delay, duration: DUR.base, easing: easeOut, css: (t, u) => `opacity: ${t}; transform: translateY(${u * y}px)` };
}

export function pageOut(_?: Element): TransitionConfig {
  if (reduced()) return { duration: 0 };
  return { duration: DUR.fast, easing: easeOut, css: (t) => `opacity: ${t}; pointer-events: none` };
}

/** "page": the sidebar stays and the content changes. "hero": a clip opens or closes. */
export type TransitionKind = "page" | "hero";

let running = 0;

/**
 * Runs `update` (which changes what's on screen) inside a view transition, or
 * just runs it when the API is missing, motion is reduced or the window isn't
 * showing. Svelte's DOM is flushed before the new picture is taken.
 *   viewTransition(() => (open = true));
 */
export async function viewTransition(update: () => void | Promise<void>, kind: TransitionKind = "page"): Promise<void> {
  if (!document.startViewTransition || reduced() || document.visibilityState !== "visible") {
    await update();
    return;
  }
  const id = ++running;
  const root = document.documentElement;
  root.dataset.vt = kind;
  const transition = document.startViewTransition(async () => {
    await update();
    flushSync();
    await tick();
  });
  // A skipped transition rejects these; the update still ran.
  transition.ready.catch(() => {});
  await transition.finished.catch(() => {});
  if (id === running) delete root.dataset.vt;
}
