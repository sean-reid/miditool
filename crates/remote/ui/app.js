/* miditool remote client. One WebSocket, no frameworks.
 *
 * Server pushes:  {type:"status", scenes, active, dropped}
 *                 {type:"events", events:[{t_ms, kind, ch, detail}]}
 * We send:        {type:"set_scene", idx}   {type:"panic"}
 */
"use strict";

(() => {
  const byId = (id) => document.querySelector(`[data-testid="${id}"]`);
  const dot = byId("conn-dot");
  const connLabel = byId("conn-label");
  const droppedEl = byId("dropped");
  const scenesEl = byId("scenes");
  const monitorEl = byId("monitor");
  const panicBtn = byId("panic");

  const MAX_ROWS = 100;
  const BACKOFF_START_MS = 250;
  const BACKOFF_MAX_MS = 5000;

  let ws = null;
  let backoff = BACKOFF_START_MS;
  let scenes = [];
  let activeIdx = -1; // last server-confirmed active scene
  let optimisticIdx = -1; // shown from tap until the next status push
  let paused = false; // finger on the monitor: hold new rows
  let held = []; // rows buffered while paused

  /* ---- connection ------------------------------------------------- */

  function connect() {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    ws = new WebSocket(`${proto}://${location.host}/ws`);
    ws.onopen = () => {
      backoff = BACKOFF_START_MS;
      setConnected(true);
    };
    ws.onclose = () => {
      setConnected(false);
      setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, BACKOFF_MAX_MS);
    };
    ws.onmessage = (e) => {
      const msg = JSON.parse(e.data);
      if (msg.type === "status") onStatus(msg);
      else if (msg.type === "events") onEvents(msg.events);
    };
  }

  function setConnected(open) {
    dot.dataset.state = open ? "open" : "closed";
    connLabel.hidden = open;
    document.body.classList.toggle("disconnected", !open);
  }

  function send(msg) {
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
  }

  /* ---- status and scenes ------------------------------------------ */

  function onStatus(status) {
    scenes = status.scenes;
    activeIdx = status.active;
    optimisticIdx = -1; // the server has spoken
    droppedEl.hidden = !(status.dropped > 0);
    if (status.dropped > 0) droppedEl.textContent = `${status.dropped} dropped`;
    renderScenes();
  }

  function renderScenes() {
    while (scenesEl.children.length > scenes.length) scenesEl.lastChild.remove();
    scenes.forEach((name, idx) => {
      let btn = scenesEl.children[idx];
      if (!btn) {
        btn = document.createElement("button");
        btn.type = "button";
        btn.className = "scene";
        btn.dataset.testid = "scene";
        btn.addEventListener("click", () => tapScene(idx));
        scenesEl.append(btn);
      }
      if (btn.textContent !== name) btn.textContent = name;
      paintScene(btn, idx);
    });
  }

  function paintScene(btn, idx) {
    const shown = optimisticIdx >= 0 ? optimisticIdx : activeIdx;
    btn.classList.toggle("active", idx === shown);
    btn.classList.toggle("optimistic", idx === optimisticIdx && idx !== activeIdx);
    // data-active reflects only what the server confirmed; tests and
    // debugging can tell it apart from the optimistic highlight.
    if (idx === activeIdx) btn.dataset.active = "1";
    else delete btn.dataset.active;
  }

  function tapScene(idx) {
    optimisticIdx = idx;
    [...scenesEl.children].forEach((btn, i) => paintScene(btn, i));
    send({ type: "set_scene", idx });
  }

  /* ---- monitor ----------------------------------------------------- */

  function onEvents(events) {
    if (paused) {
      held.push(...events);
      held.splice(0, held.length - MAX_ROWS); // keep only what can show
      return;
    }
    appendRows(events);
  }

  function appendRows(events) {
    // Events arrive oldest first; prepending in order leaves the
    // newest at the top.
    for (const ev of events) monitorEl.prepend(makeRow(ev));
    while (monitorEl.children.length > MAX_ROWS) monitorEl.lastChild.remove();
  }

  function makeRow(ev) {
    const li = document.createElement("li");
    li.className = `row kind-${ev.kind}`;
    li.dataset.testid = "event-row";
    li.dataset.t = String(ev.t_ms);
    for (const [cls, text] of [
      ["t", (ev.t_ms / 1000).toFixed(2)],
      ["ch", `ch${String(ev.ch).padStart(2, "0")}`],
      ["kind", ev.kind],
      ["detail", ev.detail],
    ]) {
      const span = document.createElement("span");
      span.className = cls;
      span.textContent = text;
      li.append(span);
    }
    return li;
  }

  // A finger on the panel means the player is reading: hold new rows
  // until it lifts, then flush.
  monitorEl.addEventListener("pointerdown", () => {
    paused = true;
  });
  for (const type of ["pointerup", "pointercancel"]) {
    window.addEventListener(type, () => {
      if (!paused) return;
      paused = false;
      appendRows(held);
      held = [];
    });
  }

  /* ---- panic ------------------------------------------------------- */

  panicBtn.addEventListener("click", () => {
    send({ type: "panic" });
    panicBtn.classList.remove("flash");
    void panicBtn.offsetWidth; // restart the animation
    panicBtn.classList.add("flash");
  });

  connect();
})();
