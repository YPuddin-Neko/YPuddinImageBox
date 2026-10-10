import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const script = readFileSync(new URL("../src-tauri/src/x_capture.js", import.meta.url), "utf8");
const config = { page: "/artist/media", start: "开始自动下拉", pause: "暂停自动下拉" };
const photo = '{"media_url_https":"https://pbs.twimg.com/media/photo.jpg"}';

function browser({ url = "https://x.com/artist/media", body = true, frame = false } = {}) {
  class Target {
    listeners = new Map();
    addEventListener(type, listener) {
      if (!this.listeners.has(type)) this.listeners.set(type, new Set());
      this.listeners.get(type).add(listener);
    }
    removeEventListener(type, listener) { this.listeners.get(type)?.delete(listener); }
    emit(type, data = {}) {
      const event = { composedPath: () => [this], ...data };
      for (const listener of [...(this.listeners.get(type) || [])]) listener(event);
    }
  }
  class Element extends Target {
    constructor(tag) {
      super();
      this.tag = tag;
      this.children = [];
      this.style = {};
      this.attributes = {};
      this.parent = null;
      this.hidden = false;
    }
    get isConnected() { return this === document.documentElement || !!this.parent?.isConnected; }
    append(...children) { for (const child of children) { child.parent = this; this.children.push(child); } }
    remove() {
      if (this.parent) this.parent.children = this.parent.children.filter((child) => child !== this);
      this.parent = null;
    }
    attachShadow() { this.shadowRoot = new Element("shadow-root"); return this.shadowRoot; }
    setAttribute(name, value) { this.attributes[name] = value; }
  }
  const document = new Target();
  document.documentElement = new Element("html");
  document.documentElement.scrollHeight = 3000;
  document.createElement = (tag) => new Element(tag);
  document.visibilityState = "visible";
  const ready = () => {
    document.body = new Element("body");
    document.documentElement.append(document.body);
    document.emit("DOMContentLoaded");
  };
  if (body) ready();
  const location = {};
  const go = (value) => {
    const next = new URL(value, location.href || url);
    for (const key of ["href", "hostname", "pathname", "search", "hash"]) location[key] = next[key];
  };
  go(url);
  const history = {
    pushState(_state, _title, value) { if (value !== undefined) go(value); },
    replaceState(_state, _title, value) { if (value !== undefined) go(value); },
  };
  const timers = new Map();
  let timerId = 0;
  const observers = new Set();
  class MutationObserver {
    constructor(callback) { this.callback = callback; this.connected = false; observers.add(this); }
    observe() { this.connected = true; }
    disconnect() { this.connected = false; }
  }
  class XMLHttpRequest extends Target {
    open(...args) { this.openArgs = args; return "opened"; }
  }
  const xhrOpen = XMLHttpRequest.prototype.open;
  const scrolls = [];
  const invokes = [];
  const fetches = [];
  const response = { ok: true, clone: () => ({ text: async () => photo }) };
  const originalFetch = async function(...args) { fetches.push({ receiver: this, args }); return response; };
  const window = new Target();
  window.top = frame ? {} : window;
  window.fetch = originalFetch;
  window.scrollTo = (...args) => scrolls.push(args);
  window.__TAURI_INTERNALS__ = { invoke: async (...args) => invokes.push(args) };
  const context = vm.createContext({
    window, document, location, history, XMLHttpRequest, MutationObserver, URL, Request,
    setInterval: (callback, delay) => { timers.set(++timerId, { callback, delay }); return timerId; },
    clearInterval: (id) => timers.delete(id),
  });
  const inject = () => vm.runInContext(script, context);
  const configure = (value = config) => window.__IMAGEBOX_X_CAPTURE__.configure(value);
  const control = () => document.body?.children.find((node) => node.id === "imagebox-x-scroll-control");
  const button = () => control()?.shadowRoot.children.find((node) => node.tag === "button");
  const tick = (count = 1) => {
    for (let n = 0; n < count; n++) for (const { callback } of [...timers.values()]) callback();
  };
  const mutations = () => { for (const observer of observers) if (observer.connected) observer.callback(); };
  inject();
  return { window, document, location, history, XMLHttpRequest, xhrOpen, response, originalFetch, scrolls, invokes,
    fetches, timers, observers, inject, configure, control, button, tick, mutations, ready, go };
}

test("a video-first media page stays still until explicitly started, with restartable pause", () => {
  const b = browser();
  b.configure();
  b.tick(30);
  assert.equal(b.scrolls.length, 0);
  assert.equal(b.timers.size, 0);
  assert.equal(b.button().textContent, config.start);
  b.button().emit("click");
  assert.equal(b.timers.size, 1);
  assert.equal([...b.timers.values()][0].delay, 1500);
  assert.equal(b.button().attributes["aria-pressed"], "true");
  assert.equal(b.button().textContent, config.pause);
  b.tick(2);
  assert.deepEqual(b.scrolls, [[0, 3000], [0, 3000]]);
  b.button().emit("click");
  b.tick(5);
  assert.equal(b.scrolls.length, 2);
  assert.equal(b.timers.size, 0);
  b.button().emit("click");
  b.tick();
  assert.equal(b.scrolls.length, 3);
});

test("page interaction pauses same-URL filter changes; interaction with the control does not", () => {
  const b = browser();
  b.configure();
  for (const event of ["pointerdown", "keydown"]) {
    b.button().emit("click");
    b.document.emit(event, { composedPath: () => [b.button(), b.control()] });
    assert.equal(b.timers.size, 1);
    b.document.emit(event);
    assert.equal(b.timers.size, 0);
  }
  b.button().emit("click");
  b.document.emit("wheel", { deltaY: -1 });
  assert.equal(b.timers.size, 0);
  b.tick(20);
  assert.equal(b.scrolls.length, 0);
});

test("path, query, hash and history navigation all pause and do not resume on return", () => {
  const b = browser();
  b.configure();
  for (const [method, value] of [["pushState", "/artist/media?filter=images"], ["replaceState", "/artist/media#photos"], ["pushState", "/home"]]) {
    b.button().emit("click");
    b.history[method]({}, "", value);
    assert.equal(b.timers.size, 0);
    b.tick();
    assert.equal(b.scrolls.length, 0);
  }
  assert.equal(b.control().hidden, true);
  b.history.pushState({}, "", "/artist/media");
  assert.equal(b.control().hidden, false);
  assert.equal(b.timers.size, 0);
  for (const event of ["popstate", "hashchange"]) {
    b.button().emit("click");
    b.window.emit(event);
    assert.equal(b.timers.size, 0);
  }
  b.button().emit("click");
  b.go("/artist/media?other=1");
  b.tick();
  assert.equal(b.timers.size, 0);
  assert.equal(b.scrolls.length, 0);
});

test("login, other users, status pages and wrong collection paths cannot start scrolling", () => {
  for (const path of ["/i/flow/login", "/someone/media", "/artist/status/1", "/artist", "/artist/likes"]) {
    const b = browser({ url: "https://x.com" + path });
    b.configure();
    assert.equal(b.control().hidden, true, path);
    b.button().emit("click");
    b.tick(20);
    assert.equal(b.timers.size, 0, path);
    assert.equal(b.scrolls.length, 0, path);
  }
  for (const page of ["/artist/likes", "/i/bookmarks"]) {
    const b = browser({ url: "https://x.com" + page });
    b.configure({ ...config, page });
    b.button().emit("click");
    b.tick();
    assert.equal(b.scrolls.length, 1, page);
  }
});

test("modifier keys and switching apps retain the chosen scroll state", () => {
  const b = browser();
  b.configure();
  b.button().emit("click");
  for (const key of ["Meta", "Control", "Alt", "Shift", "CapsLock"]) b.document.emit("keydown", { key });
  b.document.emit("keydown", { key: "Tab", metaKey: true });
  assert.equal(b.timers.size, 1);
  b.tick();
  assert.equal(b.scrolls.length, 1);
  b.document.visibilityState = "hidden";
  b.document.emit("visibilitychange");
  b.tick(20);
  assert.equal(b.timers.size, 1);
  assert.equal(b.scrolls.length, 1);
  b.document.visibilityState = "visible";
  b.document.emit("visibilitychange");
  b.tick();
  assert.equal(b.scrolls.length, 2);
  b.document.emit("keydown", { key: "ArrowUp" });
  assert.equal(b.timers.size, 0);
});

test("stable height stops scrolling and a new start resets the end counter", () => {
  const b = browser();
  b.configure();
  b.button().emit("click");
  b.tick(13);
  assert.equal(b.scrolls.length, 13);
  assert.equal(b.timers.size, 0);
  b.button().emit("click");
  b.document.documentElement.scrollHeight = 6000;
  b.tick(2);
  assert.equal(b.timers.size, 1);
  assert.deepEqual(b.scrolls.at(-1), [0, 6000]);
});

test("document_start waits for body, then recovers a removed control in paused state", () => {
  const b = browser({ body: false });
  b.configure();
  assert.equal(b.control(), undefined);
  b.ready();
  assert.equal(b.button().textContent, config.start);
  b.button().emit("click");
  b.control().remove();
  b.mutations();
  assert.equal(b.button().textContent, config.start);
  assert.equal(b.timers.size, 0);
  assert.equal(b.document.body.children.length, 1);
});

test("pagehide clears the timer and observer; bfcache restoration remains paused", () => {
  const b = browser();
  b.configure();
  b.button().emit("click");
  b.window.emit("pagehide");
  b.tick(30);
  assert.equal(b.timers.size, 0);
  assert.equal(b.control(), undefined);
  assert.equal([...b.observers].some((observer) => observer.connected), false);
  b.window.emit("pageshow");
  assert.equal(b.button().textContent, config.start);
  assert.equal(b.scrolls.length, 0);
  assert.equal(b.timers.size, 0);
});

test("re-injection disposes old hooks, timers and controls without duplicating collection", async () => {
  const b = browser();
  b.configure();
  b.button().emit("click");
  const previous = b.window.__IMAGEBOX_X_CAPTURE__;
  b.inject();
  b.configure();
  assert.equal(b.timers.size, 0);
  assert.equal(b.document.body.children.length, 1);
  assert.equal(b.document.listeners.get("pointerdown").size, 1);
  previous.configure(config);
  assert.equal(b.document.body.children.length, 1);
  await b.window.fetch("https://api.x.com/graphql/id/mediaQuery");
  await Promise.resolve();
  assert.equal(b.fetches.length, 1);
  assert.equal(b.invokes.length, 1);
  b.button().emit("click");
  b.window.__IMAGEBOX_X_CAPTURE__.dispose();
  assert.equal(b.timers.size, 0);
  assert.equal(b.control(), undefined);
  assert.equal(b.window.fetch, b.originalFetch);
  assert.equal(b.XMLHttpRequest.prototype.open, b.xhrOpen);
  assert.equal(b.document.listeners.get("pointerdown").size, 0);
});

test("fetch preserves inputs, receiver and response, collects while paused, and deduplicates", async () => {
  const b = browser();
  b.configure();
  const url = new URL("https://api.x.com/graphql/id/mediaQuery");
  const options = { method: "POST", body: "query" };
  const response = await b.window.fetch(url, options);
  await Promise.resolve();
  assert.equal(response, b.response);
  assert.equal(b.fetches[0].receiver, b.window);
  assert.equal(b.fetches[0].args[0], url);
  assert.equal(b.fetches[0].args[1], options);
  assert.equal(b.invokes[0][0], "plugin:x|bridge_response");
  assert.equal(b.invokes[0][1].payload.page, "/artist/media");
  assert.equal(b.invokes[0][1].payload.body, photo);
  await b.window.fetch(new Request(url));
  await b.window.fetch("https://api.x.com/not-graphql");
  await Promise.resolve();
  assert.equal(b.invokes.length, 1);
  assert.equal(b.scrolls.length, 0);
});

test("late response bodies after navigation or disposal cannot submit an old document", async () => {
  for (const leave of [(b) => b.window.emit("pagehide"), (b) => b.window.__IMAGEBOX_X_CAPTURE__.dispose()]) {
    const b = browser();
    let finish;
    const pending = new Promise((resolve) => { finish = resolve; });
    b.response.clone = () => ({ text: () => pending });
    await b.window.fetch("https://api.x.com/graphql/id/mediaQuery");
    leave(b);
    finish(photo);
    await Promise.resolve();
    assert.equal(b.invokes.length, 0);
  }
});

test("XHR preserves open arguments and collects successful GraphQL responses only", () => {
  const b = browser();
  const xhr = new b.XMLHttpRequest();
  const url = "https://api.x.com/graphql/id/mediaQuery";
  assert.equal(xhr.open("GET", url, true), "opened");
  assert.deepEqual(xhr.openArgs, ["GET", url, true]);
  xhr.status = 403;
  xhr.responseText = photo;
  xhr.emit("load");
  assert.equal(b.invokes.length, 0);
  xhr.status = 200;
  xhr.emit("load");
  assert.equal(b.invokes.length, 1);
  xhr.open("GET", "https://x.com/home");
  xhr.responseText = photo + " ";
  xhr.emit("load");
  assert.equal(b.invokes.length, 1);
  xhr.open("GET", url);
  b.window.__IMAGEBOX_X_CAPTURE__.dispose();
  xhr.emit("load");
  assert.equal(b.invokes.length, 1);
});

test("configuration switches target and language without carrying running state", () => {
  const b = browser();
  b.configure();
  b.button().emit("click");
  b.configure({ page: "/i/bookmarks", start: "Start auto-scroll", pause: "Pause auto-scroll" });
  assert.equal(b.timers.size, 0);
  assert.equal(b.control().hidden, true);
  b.history.pushState({}, "", "/i/bookmarks");
  assert.equal(b.control().hidden, false);
  assert.equal(b.button().textContent, "Start auto-scroll");
  b.button().emit("click");
  assert.equal(b.button().textContent, "Pause auto-scroll");
});

test("third-party login documents and child frames receive no hooks or controls", () => {
  for (const options of [{ url: "https://accounts.google.com/login" }, { frame: true }]) {
    const b = browser(options);
    assert.equal(b.window.__IMAGEBOX_X_CAPTURE__, undefined);
    assert.equal(b.window.fetch, b.originalFetch);
    assert.equal(b.timers.size, 0);
  }
});
