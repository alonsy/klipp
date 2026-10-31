// One shared tooltip for the whole window. App installs it once; after that any
// element with a title="" (or data-tip="") shows it on hover and on keyboard
// focus, so nothing else is needed. A trailing "(F)" is drawn as a key. A tip
// that comes and goes uses data-tip: a title is moved off the element when it's
// first shown, so taking it away later wouldn't reach the tooltip.
const DELAY_MS = 350;
/** Moving from one control to the next shows the next tip at once. */
const WARM_MS = 400;
const GAP = 8;

export function installTooltips(): () => void {
  const el = document.createElement("div");
  el.className = "tooltip";
  el.setAttribute("role", "tooltip");
  let target: HTMLElement | null = null;
  let timer = 0;
  let hiddenAt = 0;

  /** Native title tooltips would show too: the text moves to data-tip. */
  function adopt(t: HTMLElement) {
    const title = t.getAttribute("title");
    if (title == null) return;
    t.removeAttribute("title");
    t.dataset.tip = title;
    if (!t.hasAttribute("aria-label") && !t.textContent?.trim()) t.setAttribute("aria-label", title);
  }

  function fill(text: string) {
    const key = /^(.*\S)\s+\(([^()\s]{1,6})\)$/s.exec(text);
    el.replaceChildren();
    const words = document.createElement("span");
    words.textContent = key ? key[1] : text;
    el.append(words);
    if (key) {
      const kbd = document.createElement("kbd");
      kbd.textContent = key[2];
      el.append(kbd);
    }
  }

  function place() {
    if (!target) return;
    const r = target.getBoundingClientRect();
    const tip = el.getBoundingClientRect();
    const below = r.bottom + GAP + tip.height <= innerHeight;
    const y = Math.max(GAP, below ? r.bottom + GAP : r.top - GAP - tip.height);
    const x = Math.max(GAP, Math.min(r.left + r.width / 2 - tip.width / 2, innerWidth - tip.width - GAP));
    el.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
  }

  function show() {
    if (!target?.isConnected) return;
    const text = target.dataset.tip;
    if (!text) return;
    fill(text);
    // In fullscreen, only what's inside the fullscreen element shows.
    (document.fullscreenElement ?? document.body).append(el);
    place();
    el.classList.add("show");
  }

  function hide() {
    clearTimeout(timer);
    if (el.classList.contains("show")) hiddenAt = performance.now();
    el.classList.remove("show");
    target = null;
  }

  function aim(t: HTMLElement) {
    adopt(t);
    if (t === target) return;
    clearTimeout(timer);
    target = t;
    if (performance.now() - hiddenAt < WARM_MS || el.classList.contains("show")) show();
    else timer = window.setTimeout(show, DELAY_MS);
  }

  const found = (e: Event) => (e.target as Element | null)?.closest?.<HTMLElement>("[title], [data-tip]") ?? null;

  const over = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    const t = found(e);
    if (t) aim(t);
    else if (target) hide();
  };
  const focus = (e: FocusEvent) => {
    // The focus can be on a control inside the tip's element (a switch in its row).
    const t = found(e);
    if (t && (e.target as Element).matches(":focus-visible")) aim(t);
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "Escape") hide();
  };

  document.addEventListener("pointerover", over);
  document.addEventListener("focusin", focus);
  document.addEventListener("focusout", hide);
  document.addEventListener("pointerdown", hide, true);
  document.addEventListener("scroll", hide, true);
  document.addEventListener("keydown", key);
  return () => {
    hide();
    el.remove();
    document.removeEventListener("pointerover", over);
    document.removeEventListener("focusin", focus);
    document.removeEventListener("focusout", hide);
    document.removeEventListener("pointerdown", hide, true);
    document.removeEventListener("scroll", hide, true);
    document.removeEventListener("keydown", key);
  };
}
