(() => {
  if (window.top !== window || !/^(x\.com|[^/]+\.x\.com|twitter\.com|[^/]+\.twitter\.com)$/.test(location.hostname)) return;
  window.__IMAGEBOX_X_CAPTURE__?.dispose();

  let disposed = false;
  let suspended = false;
  let config = null;
  let timer = null;
  let runningUrl = "";
  let lastHeight = 0;
  let stable = 0;
  let host = null;
  let button = null;
  const listeners = [];
  const listen = (target, type, handler, options) => {
    target.addEventListener(type, handler, options);
    listeners.push(() => target.removeEventListener(type, handler, options));
  };
  const onTarget = () => config && location.pathname.replace(/\/$/, "").toLowerCase() === config.page.toLowerCase();
  const render = () => {
    if (!host) return;
    host.hidden = !onTarget() || suspended;
    button.textContent = timer === null ? config.start : config.pause;
    button.setAttribute("aria-pressed", String(timer !== null));
  };
  const pause = () => {
    if (timer !== null) clearInterval(timer);
    timer = null;
    runningUrl = "";
    stable = 0;
    lastHeight = 0;
    render();
  };
  const tick = () => {
    if (disposed || suspended || !onTarget() || location.href !== runningUrl) {
      pause();
      return;
    }
    if (document.visibilityState === "hidden") return;
    const height = document.documentElement.scrollHeight;
    window.scrollTo(0, height);
    stable = height === lastHeight ? stable + 1 : 0;
    lastHeight = height;
    if (stable >= 12) pause();
  };
  const toggle = () => {
    if (timer !== null) {
      pause();
    } else if (!disposed && !suspended && onTarget() && document.visibilityState !== "hidden") {
      runningUrl = location.href;
      timer = setInterval(tick, 1500);
      render();
    }
  };
  const mount = () => {
    if (disposed || suspended || !config || !document.body) return;
    if (host?.isConnected) return;
    // X 会重建页面节点；控制按钮被移除后重新挂载，但不继续滚动。
    if (host) pause();
    host = document.createElement("div");
    host.id = "imagebox-x-scroll-control";
    host.style.cssText = "position:fixed;right:20px;bottom:20px;z-index:2147483647;color-scheme:light dark";
    const shadow = host.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = "button{font:600 13px/1.4 system-ui,sans-serif;color:#0f1419;background:#fff;border:1px solid #cfd9de;border-radius:999px;padding:9px 15px;box-shadow:0 2px 8px #0002;cursor:pointer}button:hover{background:#eff3f4}button:focus-visible{outline:2px solid #1d9bf0;outline-offset:3px}@media(prefers-color-scheme:dark){button{color:#e7e9ea;background:#16181c;border-color:#536471}button:hover{background:#202327}}";
    button = document.createElement("button");
    button.type = "button";
    button.addEventListener("click", toggle);
    shadow.append(style, button);
    document.body.append(host);
    render();
  };
  const observer = new MutationObserver(mount);
  const observe = () => {
    if (document.documentElement) observer.observe(document.documentElement, { childList: true, subtree: true });
    mount();
  };
  const navigated = () => {
    pause();
    mount();
  };
  const originalPushState = history.pushState;
  const originalReplaceState = history.replaceState;
  const wrapHistory = (original) => function() {
    const before = location.href;
    const result = original.apply(this, arguments);
    if (location.href !== before) navigated();
    return result;
  };
  const pushState = history.pushState = wrapHistory(originalPushState);
  const replaceState = history.replaceState = wrapHistory(originalReplaceState);
  listen(window, "popstate", navigated);
  listen(window, "hashchange", navigated);
  listen(document, "DOMContentLoaded", observe);
  const userInteraction = (event) => {
    if (!event.composedPath().includes(host)) pause();
  };
  // 筛选可能只更新同一路径；用户操作页面时先把滚动交还给用户。
  listen(document, "pointerdown", userInteraction, true);
  listen(document, "keydown", (event) => {
    if (["Meta", "Control", "Alt", "Shift", "CapsLock"].includes(event.key) || event.metaKey || event.ctrlKey || event.altKey) return;
    userInteraction(event);
  }, true);
  listen(document, "wheel", (event) => {
    if (event.deltaY < 0) pause();
  }, { passive: true });
  listen(window, "pagehide", () => {
    pause();
    suspended = true;
    observer.disconnect();
    host?.remove();
  });
  listen(window, "pageshow", () => {
    suspended = false;
    pause();
    observe();
  });

  const graphql = /\/graphql\/[^/]+\/[^/]+$/;
  const urlOf = (input) => {
    try {
      return new URL(input instanceof Request ? input.url : String(input), location.href);
    } catch (_) {
      return null;
    }
  };
  const sent = new Set();
  const send = (url, body) => {
    try {
      if (disposed || suspended || !url || !graphql.test(url.pathname) || !body || body.indexOf("media_url_https") < 0) return;
      const key = url.pathname + ":" + body.length + ":" + body.slice(0, 32);
      if (sent.has(key)) return;
      sent.add(key);
      if (sent.size > 300) sent.delete(sent.values().next().value);
      const invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
      if (typeof invoke === "function") {
        invoke("plugin:x|bridge_response", { payload: { path: url.pathname, page: location.pathname, body } }).catch(() => {});
      }
    } catch (_) {}
  };
  const originalOpen = XMLHttpRequest.prototype.open;
  const xhrListeners = new WeakMap();
  const open = XMLHttpRequest.prototype.open = function(method, url) {
    const previous = xhrListeners.get(this);
    if (previous) this.removeEventListener("load", previous);
    const parsed = urlOf(url);
    if (parsed && graphql.test(parsed.pathname)) {
      const loaded = () => {
        if (this.status === 200 && typeof this.responseText === "string") send(parsed, this.responseText);
      };
      xhrListeners.set(this, loaded);
      this.addEventListener("load", loaded);
    }
    return originalOpen.apply(this, arguments);
  };
  const originalFetch = window.fetch;
  const fetch = window.fetch = async function(input) {
    const response = await originalFetch.apply(this, arguments);
    const parsed = urlOf(input);
    if (parsed && graphql.test(parsed.pathname) && response.ok) {
      response.clone().text().then(body => send(parsed, body)).catch(() => {});
    }
    return response;
  };

  window.__IMAGEBOX_X_CAPTURE__ = {
    configure(next) {
      if (disposed) return;
      pause();
      config = next;
      observe();
      render();
    },
    dispose() {
      if (disposed) return;
      pause();
      disposed = true;
      observer.disconnect();
      listeners.forEach((remove) => remove());
      host?.remove();
      if (window.fetch === fetch) window.fetch = originalFetch;
      if (XMLHttpRequest.prototype.open === open) XMLHttpRequest.prototype.open = originalOpen;
      if (history.pushState === pushState) history.pushState = originalPushState;
      if (history.replaceState === replaceState) history.replaceState = originalReplaceState;
    },
  };
})();
