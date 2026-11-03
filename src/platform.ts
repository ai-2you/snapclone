/** Whether the app runs on macOS. WebView2 on Windows always reports "Windows NT" here, so
 * this needs no round-trip to Rust. */
export const IS_MAC = navigator.userAgent.includes("Mac");

/** The app's "primary" shortcut modifier: ⌘ on macOS, Ctrl on Windows. */
export function isPrimaryModifier(event: KeyboardEvent | MouseEvent): boolean {
  return IS_MAC ? event.metaKey : event.ctrlKey;
}

/**
 * The OTHER platform's primary modifier, which this app never uses as a shortcut modifier:
 * Ctrl on macOS, Win on Windows (`event.metaKey` there). Exposed for conditions that mean
 * "no modifier besides the primary one" — substituting `isPrimaryModifier` alone into such a
 * condition without also replacing its `event.ctrlKey` check would leave that check always
 * true on Windows (Ctrl already being the primary modifier there), making the shortcut
 * impossible to press.
 */
export function isSecondaryModifier(event: KeyboardEvent | MouseEvent): boolean {
  return IS_MAC ? event.ctrlKey : event.metaKey;
}

/** How a shortcut is written in menus/tooltips: `⇧⌘S` on macOS, `Ctrl+Shift+S` on Windows. */
export function formatShortcut(key: string, mods: { shift?: boolean; alt?: boolean } = {}): string {
  if (IS_MAC) {
    return `${mods.alt ? "⌥" : ""}${mods.shift ? "⇧" : ""}⌘${key}`;
  }
  return `Ctrl+${mods.alt ? "Alt+" : ""}${mods.shift ? "Shift+" : ""}${key}`;
}

/**
 * Adds `platform-mac`/`platform-windows` to `<body>`, the single source every page uses to
 * tell the two apart in CSS. Call once, as early as possible in each page's entry script.
 */
export function applyPlatformClass(body: HTMLElement = document.body): void {
  body.classList.add(IS_MAC ? "platform-mac" : "platform-windows");
}
