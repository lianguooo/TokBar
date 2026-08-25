// Injected into the Codex desktop app's renderer by TokBar.
//
// Adds a hover-revealed delete button to every sidebar conversation. The row
// carries its own id in `data-app-action-sidebar-thread-id`, so no React
// internals are touched -- if that attribute ever goes away this script simply
// finds nothing and stays inert rather than breaking the app.
//
// Re-evaluated on every reconnect and on every navigation, so everything here
// must be idempotent.
(() => {
  const MARK = "__tokbarSessionDelete";
  const ROW_SELECTOR = "[data-app-action-sidebar-thread-id]";
  const STYLE_ID = "tokbar-delete-style";
  const BUTTON_CLASS = "tokbar-delete-button";
  // Bump whenever the markup, placement or styling below changes. Without it
  // an updated script re-evaluated into a page that already has an older one
  // would hit the idempotency guard and silently never take effect.
  const VERSION = "7";

  const previous = window[MARK];
  if (previous?.version === VERSION) {
    // Same build already here. This also runs when TokBar reconnects after
    // being closed, so it is the moment to undo a bridge-down teardown.
    previous.revive();
    return;
  }
  if (previous) {
    // Older build: tear it down completely before installing this one.
    previous.closeDialog?.();
    previous.observer?.disconnect();
    document
      .querySelectorAll(`.${BUTTON_CLASS}`)
      .forEach((button) => (button.parentElement?.classList.contains("contents")
        ? button.parentElement
        : button
      ).remove());
    document.getElementById(STYLE_ID)?.remove();
    // VERSION 4 did not expose a close hook, so also clean up its unstyled
    // confirmation node if an update lands while the dialog is open.
    document.querySelectorAll(".tokbar-overlay").forEach((node) => node.remove());
  }

  const state = {
    version: VERSION,
    scan: () => {},
    revive: () => {},
    closeDialog: null,
    observer: null,
    // Set once a call goes unanswered: TokBar is not running, so the buttons
    // cannot do anything and should not pretend otherwise.
    bridgeDown: false,
  };
  window[MARK] = state;

  const css = `
    /* Sized to match Codex's own sidebar icon buttons (19x20, round, 16px
       glyph) so it reads as one of them rather than an addition. */
    .${BUTTON_CLASS} {
      display: flex; align-items: center; justify-content: center;
      width: 19px; height: 20px; padding: 0; border: 0; border-radius: 9999px;
      background: transparent; color: inherit; cursor: pointer;
      /* Codex tints its own row icons down; match that so the delete does not
         read as heavier than pin and archive sitting next to it. */
      opacity: .55;
    }
    .${BUTTON_CLASS}:hover { opacity: 1; background: rgba(220,38,38,.14); color: #dc2626; }
    .${BUTTON_CLASS}:focus-visible {
      opacity: 1; outline: 2px solid #2563eb; outline-offset: 2px;
    }
    .${BUTTON_CLASS} svg { width: 16px; height: 16px; }

    /* Fallback only: used when the native action cluster cannot be found, so
       there is nothing to sit beside and nothing to overlap either. */
    .${BUTTON_CLASS}--floating {
      position: absolute; right: 6px; top: 50%; transform: translateY(-50%);
      display: none; z-index: 5;
    }
    ${ROW_SELECTOR}:hover .${BUTTON_CLASS}--floating,
    ${ROW_SELECTOR}:focus-within .${BUTTON_CLASS}--floating { display: flex; }

    /* The confirmation is a lightweight, modal popover anchored to the trash
       button. The transparent scrim keeps outside-click dismissal without
       visually disconnecting the prompt from the row being acted on. */
    .tokbar-overlay {
      position: fixed; inset: 0; z-index: 2147483000;
      background: transparent;
    }
    .tokbar-dialog {
      all: initial;
      position: fixed; visibility: hidden;
      box-sizing: border-box;
      width: min(320px, calc(100vw - 16px));
      padding: 14px;
      border: 1px solid rgba(127,127,127,.28);
      border-color: color-mix(in srgb, var(--tokbar-text, CanvasText) 18%, transparent);
      border-radius: 12px;
      background: var(--tokbar-surface, Canvas);
      color: var(--tokbar-text, CanvasText);
      opacity: 1; backdrop-filter: none;
      box-shadow: 0 12px 32px rgba(0,0,0,.2), 0 2px 8px rgba(0,0,0,.12);
      font-family: var(--tokbar-font, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif);
      font-size: 13px; line-height: 1.45;
      animation: tokbar-popover-in 120ms ease-out;
    }
    .tokbar-dialog[data-placement="right"] { transform-origin: left center; }
    .tokbar-dialog[data-placement="left"] { transform-origin: right center; }
    .tokbar-dialog::before {
      content: ""; position: absolute;
      top: calc(var(--tokbar-arrow-y, 20px) - 5px);
      width: 10px; height: 10px;
      background: var(--tokbar-surface, Canvas);
      transform: rotate(45deg);
    }
    .tokbar-dialog[data-placement="right"]::before {
      left: -6px;
      border-left: 1px solid color-mix(in srgb, var(--tokbar-text, CanvasText) 18%, transparent);
      border-bottom: 1px solid color-mix(in srgb, var(--tokbar-text, CanvasText) 18%, transparent);
    }
    .tokbar-dialog[data-placement="left"]::before {
      right: -6px;
      border-right: 1px solid color-mix(in srgb, var(--tokbar-text, CanvasText) 18%, transparent);
      border-top: 1px solid color-mix(in srgb, var(--tokbar-text, CanvasText) 18%, transparent);
    }
    .tokbar-dialog[data-placement="center"]::before { display: none; }
    .tokbar-dialog h2,
    .tokbar-dialog p { all: unset; display: block; color: inherit; }
    .tokbar-dialog h2 { font-size: 14px; font-weight: 600; line-height: 1.4; }
    .tokbar-dialog p {
      margin-top: 6px;
      color: color-mix(in srgb, var(--tokbar-text, CanvasText) 68%, transparent);
      overflow: hidden; overflow-wrap: anywhere;
      display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3;
    }
    .tokbar-actions {
      display: flex; align-items: center; justify-content: flex-end;
      gap: 8px; margin-top: 12px;
    }
    .tokbar-dialog button {
      all: unset; box-sizing: border-box;
      display: inline-flex; align-items: center; justify-content: center;
      min-width: 56px; height: 32px; padding: 0 12px;
      border-radius: 7px; cursor: pointer; user-select: none;
      font-family: inherit; font-size: 13px; font-weight: 500; line-height: 1;
    }
    .tokbar-dialog button:focus-visible {
      outline: 2px solid #2563eb; outline-offset: 2px;
    }
    .tokbar-dialog [data-cancel] {
      background: color-mix(in srgb, var(--tokbar-text, CanvasText) 7%, transparent);
    }
    .tokbar-dialog [data-cancel]:hover {
      background: color-mix(in srgb, var(--tokbar-text, CanvasText) 12%, transparent);
    }
    .tokbar-dialog [data-confirm] { background: #dc2626; color: #fff; }
    .tokbar-dialog [data-confirm]:hover { background: #b91c1c; }

    .tokbar-toast {
      position: fixed; left: 50%; bottom: 24px; z-index: 2147483001;
      display: flex; align-items: center; gap: 12px;
      box-sizing: border-box; max-width: min(420px, calc(100vw - 24px));
      padding: 10px 12px; border-radius: 10px;
      background: #18181b; color: #fafafa;
      box-shadow: 0 10px 30px rgba(0,0,0,.24);
      transform: translateX(-50%);
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      font-size: 13px; line-height: 1.4;
      animation: tokbar-toast-in 150ms ease-out;
    }
    .tokbar-toast span { overflow-wrap: anywhere; }
    .tokbar-toast button {
      all: unset; flex: none; cursor: pointer; color: #fca5a5; font-weight: 600;
    }
    .tokbar-toast button:hover { color: #fecaca; }
    .tokbar-toast button:focus-visible {
      outline: 2px solid #fafafa; outline-offset: 3px; border-radius: 3px;
    }

    @keyframes tokbar-popover-in {
      from { opacity: .92; transform: scale(.98); }
      to { opacity: 1; transform: scale(1); }
    }
    @keyframes tokbar-toast-in {
      from { opacity: 0; transform: translate(-50%, 4px); }
      to { opacity: 1; transform: translate(-50%, 0); }
    }
    @media (prefers-reduced-motion: reduce) {
      .tokbar-dialog, .tokbar-toast { animation: none; }
    }
  `;

  function ensureStyle() {
    let style = document.getElementById(STYLE_ID);
    if (style) return;
    style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = css;
    document.head?.appendChild(style);
  }

  function trashIcon() {
    // 1.5 stroke to sit at the same weight as Codex's own icons.
    return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"
      stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v6M14 11v6"/></svg>`;
  }

  function rowTitle(row) {
    const node = row.querySelector("[data-thread-title], .truncate");
    return (node?.textContent || row.textContent || "").trim().slice(0, 120);
  }

  let colorContext = null;

  /** Resolve any browser-supported CSS color to RGBA. This also understands
   *  modern `color()` / `oklab()` values that a regex cannot classify. */
  function parseCssColor(value) {
    if (!colorContext) {
      const canvas = document.createElement("canvas");
      canvas.width = 1;
      canvas.height = 1;
      colorContext = canvas.getContext("2d", { willReadFrequently: true });
    }
    if (!colorContext) return null;
    colorContext.clearRect(0, 0, 1, 1);
    colorContext.fillStyle = "rgba(0, 0, 0, 0)";
    colorContext.fillStyle = value;
    colorContext.fillRect(0, 0, 1, 1);
    return [...colorContext.getImageData(0, 0, 1, 1).data];
  }

  function solidRgb(color) {
    return `rgb(${color[0]}, ${color[1]}, ${color[2]})`;
  }

  /** Find an opaque nearby surface so the popover follows Codex light/dark
   *  theme without inheriting translucent sidebar layers. */
  function anchorTheme(anchor) {
    let node = anchor;
    while (node instanceof Element) {
      const style = getComputedStyle(node);
      const background = style.backgroundColor;
      const parsed = background ? parseCssColor(background) : null;
      if (parsed && parsed[3] >= 242) {
        return { background: solidRgb(parsed), color: style.color, font: style.fontFamily };
      }
      node = node.parentElement;
    }
    const bodyStyle = getComputedStyle(document.body);
    const text = parseCssColor(bodyStyle.color);
    const darkTheme = text ? (text[0] + text[1] + text[2]) / 3 > 150 : false;
    return {
      background: darkTheme ? "rgb(32, 32, 32)" : "rgb(255, 255, 255)",
      color: bodyStyle.color || "CanvasText",
      font: bodyStyle.fontFamily,
    };
  }

  /** Keep the prompt beside its trigger, flipping sides and clamping to the
   *  viewport when the sidebar or app window is narrow. */
  function positionDialog(dialog, anchor) {
    if (!anchor.isConnected) return false;
    const anchorRect = anchor.getBoundingClientRect();
    // offset dimensions are not affected by the entry scale animation.
    const dialogWidth = dialog.offsetWidth;
    const dialogHeight = dialog.offsetHeight;
    const viewportWidth = document.documentElement.clientWidth;
    const viewportHeight = document.documentElement.clientHeight;
    const margin = 8;
    const gap = 9;
    let placement = "right";
    let left = anchorRect.right + gap;

    if (left + dialogWidth > viewportWidth - margin) {
      placement = "left";
      left = anchorRect.left - dialogWidth - gap;
    }
    if (left < margin) {
      placement = "center";
      left = Math.max(
        margin,
        Math.min(
          anchorRect.left + anchorRect.width / 2 - dialogWidth / 2,
          viewportWidth - dialogWidth - margin,
        ),
      );
    }

    const maxTop = Math.max(margin, viewportHeight - dialogHeight - margin);
    const top = Math.max(
      margin,
      Math.min(anchorRect.top + anchorRect.height / 2 - dialogHeight / 2, maxTop),
    );
    const arrowY = Math.max(
      14,
      Math.min(anchorRect.top + anchorRect.height / 2 - top, dialogHeight - 14),
    );
    dialog.dataset.placement = placement;
    dialog.style.left = `${Math.round(left)}px`;
    dialog.style.top = `${Math.round(top)}px`;
    dialog.style.setProperty("--tokbar-arrow-y", `${Math.round(arrowY)}px`);
    dialog.style.visibility = "visible";
    return true;
  }

  function confirmDelete(title, anchor) {
    return new Promise((resolve) => {
      state.closeDialog?.();
      const overlay = document.createElement("div");
      overlay.className = "tokbar-overlay";
      const safeTitle = title || "这条对话";
      overlay.innerHTML = `
        <div id="tokbar-delete-dialog" class="tokbar-dialog" role="alertdialog"
             aria-modal="true" aria-labelledby="tokbar-delete-title"
             aria-describedby="tokbar-delete-description">
          <h2 id="tokbar-delete-title">删除对话？</h2>
          <p id="tokbar-delete-description"></p>
          <div class="tokbar-actions">
            <button data-cancel>取消</button>
            <button data-confirm>删除</button>
          </div>
        </div>`;
      // textContent, never innerHTML: the title is app data, not markup.
      overlay.querySelector("p").textContent =
        `「${safeTitle}」及对应日志将被删除，可在提示条中撤销。`;
      const dialog = overlay.querySelector(".tokbar-dialog");
      const cancelButton = overlay.querySelector("[data-cancel]");
      const confirmButton = overlay.querySelector("[data-confirm]");
      const theme = anchorTheme(anchor);
      dialog.style.setProperty("--tokbar-surface", theme.background);
      dialog.style.setProperty("--tokbar-text", theme.color);
      dialog.style.setProperty("--tokbar-font", theme.font);
      anchor.setAttribute("aria-expanded", "true");
      let closed = false;
      let positionFrame = null;
      const close = (result) => {
        if (closed) return;
        closed = true;
        state.closeDialog = null;
        if (positionFrame !== null) cancelAnimationFrame(positionFrame);
        anchor.setAttribute("aria-expanded", "false");
        overlay.remove();
        document.removeEventListener("keydown", onKey, true);
        document.removeEventListener("scroll", schedulePosition, true);
        window.removeEventListener("resize", schedulePosition);
        if (!result && anchor.isConnected) anchor.focus({ preventScroll: true });
        resolve(result);
      };
      state.closeDialog = () => close(false);
      const updatePosition = () => {
        positionFrame = null;
        if (!positionDialog(dialog, anchor)) close(false);
      };
      const schedulePosition = () => {
        if (positionFrame !== null) return;
        positionFrame = requestAnimationFrame(updatePosition);
      };
      const onKey = (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          close(false);
          return;
        }
        if (event.key !== "Tab") return;
        if (event.shiftKey && document.activeElement === cancelButton) {
          event.preventDefault();
          confirmButton.focus();
        } else if (!event.shiftKey && document.activeElement === confirmButton) {
          event.preventDefault();
          cancelButton.focus();
        }
      };
      overlay.addEventListener("click", (event) => {
        if (event.target === overlay) close(false);
      });
      cancelButton.addEventListener("click", () => close(false));
      confirmButton.addEventListener("click", () => close(true));
      document.addEventListener("keydown", onKey, true);
      document.addEventListener("scroll", schedulePosition, true);
      window.addEventListener("resize", schedulePosition);
      document.body.appendChild(overlay);
      updatePosition();
      // A destructive action must not be the default keyboard choice.
      cancelButton.focus({ preventScroll: true });
    });
  }

  let toastTimer = null;

  function showToast(message, undoToken) {
    document.querySelectorAll(".tokbar-toast").forEach((node) => node.remove());
    clearTimeout(toastTimer);
    const toast = document.createElement("div");
    toast.className = "tokbar-toast";
    toast.setAttribute("role", "status");
    toast.setAttribute("aria-live", "polite");
    const label = document.createElement("span");
    label.textContent = message;
    toast.appendChild(label);
    if (undoToken) {
      const undo = document.createElement("button");
      undo.textContent = "撤销";
      undo.addEventListener("click", async () => {
        toast.remove();
        const result = await window.__tokbarInvoke("undo_delete", { token: undoToken });
        if (result?.code === "bridge_down") {
          markBridgeDown();
          return;
        }
        showToast(result?.status === "ok" ? "已恢复，重启 Codex 后显示" : (result?.message || "撤销失败"), null);
      });
      toast.appendChild(undo);
    }
    document.body.appendChild(toast);
    toastTimer = setTimeout(() => toast.remove(), undoToken ? 8000 : 4000);
  }

  /** Deleting the conversation you are looking at leaves a dead view. */
  function isViewingThread(threadId) {
    return window.location.href.includes(threadId);
  }

  /** Only persisted local rows have a rollout that can be deleted. Codex uses
   *  `local:client-new-thread:<uuid>` while a newly-created row is still
   *  optimistic; sending that id to storage can only produce "not found".
   *  Cloud/catalog rows are likewise outside this local delete feature. */
  function isPersistedLocalThread(threadId) {
    return (
      threadId?.startsWith("local:") &&
      !threadId.startsWith("local:client-new-thread:")
    );
  }

  /** TokBar has gone away: drop the buttons rather than leave dead ones. */
  function markBridgeDown() {
    state.bridgeDown = true;
    document.querySelectorAll(`.${BUTTON_CLASS}`).forEach((button) => {
      const wrapper = button.parentElement;
      (wrapper?.classList.contains("contents") ? wrapper : button).remove();
    });
    showToast("TokBar 未在运行，删除按钮已隐藏", null);
  }

  async function onDelete(row, threadId, event) {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation?.();
    if (!(await confirmDelete(rowTitle(row), event.currentTarget))) return;
    const result = await window.__tokbarInvoke("delete_thread", { threadId });
    if (result?.code === "bridge_down") {
      markBridgeDown();
      return;
    }
    if (result?.status !== "ok") {
      showToast(result?.message || "删除失败", null);
      return;
    }
    row.remove();
    showToast(result.message || "已删除", result.undoToken);
    if (isViewingThread(threadId)) window.location.reload();
  }

  // Codex keeps a row's pin/archive buttons in one flex container that its own
  // `group-hover` fades in. Joining that container is what keeps the delete
  // button beside them instead of on top of them, and it inherits the reveal,
  // spacing and alignment for free.
  function actionHost(row) {
    const native = [...row.querySelectorAll("button[aria-label]")].find(
      (button) => !button.classList.contains(BUTTON_CLASS),
    );
    if (!native) return null;
    // Each native button is wrapped in a display:contents span, so the real
    // flex parent is one level further up.
    const wrapper = native.parentElement;
    const host =
      wrapper && wrapper.classList.contains("contents") ? wrapper.parentElement : wrapper;
    return host && row.contains(host) && host !== row ? host : null;
  }

  function decorate(row) {
    if (state.bridgeDown) return;
    const threadId = row.getAttribute("data-app-action-sidebar-thread-id");
    if (!isPersistedLocalThread(threadId) || row.querySelector(`.${BUTTON_CLASS}`)) return;
    const button = document.createElement("button");
    button.type = "button";
    button.className = BUTTON_CLASS;
    button.title = "删除对话";
    button.setAttribute("aria-label", "删除对话");
    button.setAttribute("aria-haspopup", "dialog");
    button.setAttribute("aria-expanded", "false");
    button.innerHTML = trashIcon();
    // The row is a link; every pointer event has to stop before it navigates.
    for (const name of ["pointerdown", "mousedown", "click"]) {
      button.addEventListener(
        name,
        (event) => {
          if (name === "click") return onDelete(row, threadId, event);
          event.preventDefault();
          event.stopPropagation();
          event.stopImmediatePropagation?.();
        },
        true,
      );
    }
    const host = actionHost(row);
    if (host) {
      // First child: to the left of pin and archive, as requested.
      const wrapper = document.createElement("span");
      wrapper.className = "contents";
      wrapper.appendChild(button);
      host.insertBefore(wrapper, host.firstChild);
      return;
    }
    button.classList.add(`${BUTTON_CLASS}--floating`);
    row.appendChild(button);
  }

  function scan() {
    ensureStyle();
    document.querySelectorAll(ROW_SELECTOR).forEach(decorate);
  }

  state.scan = scan;
  state.revive = () => {
    state.bridgeDown = false;
    scan();
  };

  // One observer for the document: rows are virtualised, so they appear and
  // disappear constantly as the sidebar scrolls.
  let pending = null;
  state.observer = new MutationObserver(() => {
    if (pending) return;
    pending = setTimeout(() => {
      pending = null;
      scan();
    }, 120);
  });
  state.observer.observe(document.documentElement, { childList: true, subtree: true });
  scan();
})();
