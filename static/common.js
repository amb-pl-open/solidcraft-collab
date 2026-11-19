// Shared helpers for the collab pages.
const $ = (sel, el = document) => el.querySelector(sel);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

async function api(method, url, body, raw) {
  const opt = { method, headers: { "X-Collab": "1" }, credentials: "same-origin" };
  if (body !== undefined) {
    if (raw) { opt.body = body; } else { opt.body = JSON.stringify(body); opt.headers["Content-Type"] = "application/json"; }
  }
  const res = await fetch(url, opt);
  let data = null;
  const text = await res.text();
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  if (res.status === 401 && !location.pathname.endsWith("/index.html") && location.pathname !== "/") {
    location.href = "/?next=" + encodeURIComponent(location.pathname + location.search);
    throw new Error("Please sign in.");
  }
  if (res.status === 403 && data && typeof data.error === "string" && data.error.startsWith("Choose your own password") && !location.pathname.startsWith("/account.html")) {
    location.href = "/account.html?change=1";
    throw new Error(data.error);
  }
  if (!res.ok) { const e = new Error((data && data.error) || `Request failed (${res.status})`); e.status = res.status; e.data = data; throw e; }
  return data;
}

function initials(name) {
  return String(name || "?").trim().split(/\s+/).slice(0, 2).map((w) => w[0]).join("").toUpperCase();
}
function avatar(p, title) {
  return `<span class="avatar" style="background:${esc(p.color)}" title="${esc(title || p.name)}">${esc(initials(p.name))}</span>`;
}
function chip(p, text, editing) {
  return `<span class="chip ${editing ? "editing" : ""}" style="${editing ? "color:" + esc(p.color) : ""}">${avatar(p)}<span style="color:var(--ink)">${esc(text)}</span></span>`;
}
function ago(t) {
  const s = Math.max(0, Date.now() / 1000 - t);
  if (s < 60) return "just now";
  if (s < 3600) return Math.floor(s / 60) + " min ago";
  if (s < 86400) return Math.floor(s / 3600) + " h ago";
  return new Date(t * 1000).toLocaleDateString();
}
function showMsg(el, text, err) {
  el.innerHTML = text ? `<div class="msg ${err ? "err" : ""}">${esc(text)}</div>` : "";
}
function qs(name) { return new URLSearchParams(location.search).get(name); }

async function header(active) {
  const [cfg, me] = await Promise.all([api("GET", "/api/config"), api("GET", "/api/me")]);
  document.title = document.title.replace("{site}", cfg.site_name);
  const h = document.createElement("header");
  h.className = "top";
  h.innerHTML = `<a class="brand" href="/app.html">${esc(cfg.site_name)}</a>
    <nav><a href="/app.html" class="${active === "projects" ? "on" : ""}">Projects</a>
    ${me.user.is_admin ? `<a href="/admin.html" class="${active === "admin" ? "on" : ""}">Admin</a>` : ""}
    <a href="/account.html" class="${active === "account" ? "on" : ""}">Account</a></nav>
    <span class="row">${avatar(me.user)}<span>${esc(me.user.name)}</span><button id="logout">Sign out</button></span>`;
  document.body.prepend(h);
  $("#logout").onclick = async () => { await api("POST", "/api/auth/logout"); location.href = "/"; };
  return { cfg, me };
}

/// Live channel for a project: calls onEvent(msg); reconnects; send(obj) to publish presence.
function liveChannel(projectId, onEvent) {
  let ws, closed = false, queue = null;
  function open() {
    ws = new WebSocket((location.protocol === "https:" ? "wss://" : "ws://") + location.host + `/api/projects/${projectId}/live`);
    ws.onmessage = (e) => { try { onEvent(JSON.parse(e.data)); } catch {} };
    ws.onopen = () => { if (queue) ws.send(JSON.stringify(queue)); };
    ws.onclose = () => { if (!closed) setTimeout(open, 2000); };
  }
  open();
  return {
    send(obj) { queue = obj; if (ws.readyState === 1) ws.send(JSON.stringify(obj)); },
    close() { closed = true; ws.close(); },
  };
}
