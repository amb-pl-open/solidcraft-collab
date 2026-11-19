const PID = Number(qs("p"));
let PATH = qs("f");
let isNew = qs("new") === "1";
let app = null, editing = false, savedRevision = null, lastRevision = null, me = null, live = null, holder = null, detail = null;
const fileUrl = () => `/api/projects/${PID}/files/${PATH.split("/").map(encodeURIComponent).join("/")}`;
const designName = () => PATH.replace(/\.json$/i, "").split("/").pop();

function banner(text, color) {
  const b = $("#banner");
  b.hidden = !text; b.textContent = text || ""; if (color) b.style.borderColor = color;
}
function setState(t) { $("#state").textContent = t; }

async function waitForApp() {
  const frame = $("#frame");
  frame.src = "/app/?embed=editor";
  for (let i = 0; i < 240; i++) {
    await new Promise((r) => setTimeout(r, 250));
    try { const a = frame.contentWindow && frame.contentWindow.solidcraft; if (a && a.request) { await a.request("document.revision"); return a; } } catch {}
  }
  throw new Error("SolidCraft did not start in this browser. A browser with WebGPU or WebGL2 is needed.");
}

async function loadLatest() {
  const res = await fetch(fileUrl(), { cache: "no-store" });
  if (!res.ok) throw new Error("Could not read this design from the server.");
  const text = await res.text();
  await app.request("document.import", { text, name: designName() });
  const r = await app.request("document.revision");
  savedRevision = lastRevision = r.revision;
}

async function tryLock() {
  const r = await api("POST", `/api/projects/${PID}/lock`, { path: PATH });
  holder = r.lock;
  setEditing(r.granted);
  return r.granted;
}

function setEditing(on) {
  editing = on;
  $("#save").disabled = !on;
  $("#take").hidden = on;
  if (on) { banner(""); setState("Editing · only you can change this part right now"); }
  else if (holder) { banner(`${holder.user_name} is editing this part · you are viewing, and it updates when they save`, holder.color); setState("View only"); }
  sendPresence();
}

function sendPresence() {
  if (live) live.send({ type: "presence", path: PATH, mode: editing ? "editing" : "viewing", detail });
}

async function save() {
  if (!editing && !isNew) return;
  $("#save").disabled = true; setState("Saving…");
  try {
    const kept = await app.request("document.export");
    const msg = prompt("What changed? (optional, shown in the version list)", "") || "";
    const r = await api("PUT", fileUrl() + (msg ? `?message=${encodeURIComponent(msg)}` : ""), kept.text, true);
    await app.request("document.saved", { revision: kept.revision });
    savedRevision = kept.revision;
    if (isNew) { isNew = false; history.replaceState(null, "", `/edit.html?p=${PID}&f=${encodeURIComponent(PATH)}`); await tryLock(); }
    setState(`Saved version ${r.version}`);
  } catch (e) {
    setState(e.message);
    if (e.status === 409) { holder = e.data && e.data.lock; setEditing(false); }
  } finally { $("#save").disabled = !editing; }
}

/// Best effort: what the person is on, from the app's own UI state (open dialog or selection).
function summarize(ui, sel) {
  try {
    const d = ui && ui.dialog;
    if (d) {
      const name = d.title || d.header || d.name || d.command || d.id;
      if (name) return String(name);
    }
    const items = (sel && (sel.selection || sel.items)) || [];
    if (items.length) {
      const it = items[0];
      const label = it.feature_name || it.name || it.body_name || it.type;
      return items.length > 1 ? `${items.length} items selected` : `${label} selected`;
    }
    if (ui && ui.active_tool) return String(ui.active_tool);
  } catch {}
  return null;
}

async function poll() {
  if (!app) return;
  try {
    const r = await app.request("document.revision");
    lastRevision = r.revision;
    const dirty = editing && savedRevision !== null && lastRevision !== savedRevision;
    if (editing) setState(dirty ? "Editing · unsaved changes" : "Editing · all changes saved");
    const [ui, sel] = await Promise.all([app.request("ui.inspect").catch(() => null), app.request("ui.selection").catch(() => null)]);
    const d = summarize(ui, sel);
    if (d !== detail) { detail = d; sendPresence(); }
  } catch {}
}

function renderPeople(people) {
  const here = people.filter((p) => p.path === PATH && p.conn !== live.conn);
  $("#people").innerHTML = here.map((p) => chip(p, `${p.name} ${p.mode === "editing" ? "editing" : "viewing"}${p.detail ? " · " + p.detail : ""}`, p.mode === "editing")).join("");
}

(async () => {
  try {
    const m = await api("GET", "/api/me"); me = m.user;
    const cfg = await api("GET", "/api/config");
    document.title = `${PATH} · ${cfg.site_name}`;
    $("#name").textContent = PATH;
    $("#back").href = `/project.html?id=${PID}`;
    if (!cfg.editor) throw new Error("This server has no SolidCraft browser build configured (COLLAB_APP_DIR).");
    live = liveChannel(PID, async (msg) => {
      if (msg.type === "hello") live.conn = msg.conn;
      if (msg.type === "hello" || msg.type === "presence") renderPeople(msg.people);
      if (msg.type === "saved" && msg.path === PATH && msg.by.id !== me.id && !editing) {
        banner(`${msg.by.name} saved version ${msg.version} · showing it now`, msg.by.color);
        try { await loadLatest(); } catch {}
        setTimeout(() => holder && !editing && banner(`${holder.user_name} is editing this part · you are viewing, and it updates when they save`, holder.color), 4000);
      }
      if (msg.type === "locks" && !editing) {
        const l = msg.locks.find((x) => x.path === PATH);
        if (!l) { banner("This part is free now. Press 'Edit when free' to take it."); $("#take").hidden = false; }
      }
      if (msg.type === "locks" && editing && msg.forced && msg.forced.path === PATH) {
        setEditing(false); banner(`${msg.forced.by} freed this part. Your unsaved changes are still here; take the part again to save them.`);
      }
    });
    app = await waitForApp();
    if (isNew) { editing = true; $("#save").disabled = false; setState("New design · press Save version to create it"); savedRevision = (await app.request("document.revision")).revision; }
    else { await loadLatest(); await tryLock(); }
    sendPresence();
    setInterval(poll, 2000);
    setInterval(() => { if (editing && !isNew) tryLock().catch(() => {}); }, 30000);
  } catch (e) { setState(e.message); banner(e.message); }
})();

$("#save").onclick = save;
$("#take").onclick = async () => {
  try { await loadLatest(); if (!(await tryLock())) banner(`${holder.user_name} is still editing it.`, holder.color); } catch (e) { setState(e.message); }
};
window.addEventListener("beforeunload", (e) => {
  if (editing && savedRevision !== null && lastRevision !== savedRevision) { e.preventDefault(); e.returnValue = ""; }
});
window.addEventListener("pagehide", () => {
  if (editing && !isNew) fetch(`/api/projects/${PID}/lock`, { method: "DELETE", keepalive: true, headers: { "Content-Type": "application/json", "X-Collab": "1" }, body: JSON.stringify({ path: PATH }) });
});
