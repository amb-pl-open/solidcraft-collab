const PID = Number(qs("id"));
let INFO, PEOPLE = [], LOCKS = [], OPEN = null;

function feed(html) {
  const f = $("#feed");
  if (f.querySelector(".muted")) f.innerHTML = "";
  const d = document.createElement("div"); d.className = "item"; d.innerHTML = html; f.prepend(d);
}
function lockOf(path) { return LOCKS.find((l) => l.path === path); }
function peopleOn(path) { return PEOPLE.filter((p) => p.path === path); }

function renderHere() {
  const uniq = PEOPLE.filter((x, i, a) => a.findIndex((y) => y.user_id === x.user_id) === i);
  $("#here").innerHTML = uniq.map((p) => {
    const where = p.path ? `${p.mode === "editing" ? "editing" : "viewing"} ${p.path}${p.detail ? " · " + p.detail : ""}` : "looking at the project";
    return chip(p, `${p.name} — ${where}`, p.mode === "editing");
  }).join("");
}

function renderFiles() {
  const rows = INFO.files.map((f) => {
    const l = lockOf(f.path);
    const viewers = peopleOn(f.path).filter((p) => !l || p.user_id !== l.user_id);
    const ed = l ? (() => { const p = PEOPLE.find((x) => x.user_id === l.user_id && x.path === f.path); return chip({ name: l.user_name, color: l.color }, `${l.user_name} editing${p && p.detail ? " · " + p.detail : ""}`, true); })() : "";
    const vw = viewers.length ? `<span class="avatars">${viewers.map((p) => avatar(p, p.name + " is viewing")).join("")}</span>` : "";
    const open = f.kind === "design" && INFO.editor ? `<a class="btn" href="/edit.html?p=${PID}&f=${encodeURIComponent(f.path)}">Open</a>` : "";
    const dl = `<a class="btn" href="/api/projects/${PID}/files/${encodeURIComponent(f.path).replace(/%2F/g, "/")}" download>Download</a>`;
    const force = l && INFO.can_manage && l.user_id !== INFO.me.id ? `<button data-force="${esc(f.path)}" class="danger" title="Free this part so someone else can edit it">Free it</button>` : "";
    return `<tr><td><strong>${esc(f.path)}</strong><br><span class="muted">${f.kind} · ${(f.size / 1024).toFixed(0)} KB · ${ago(f.modified)}</span></td>
      <td><div class="row">${ed}${vw}</div></td>
      <td><div class="row">${open}<button data-detail="${esc(f.path)}">History & comments</button>${dl}${force}</div></td></tr>`;
  });
  $("#files").innerHTML = rows.join("") || `<tr><td colspan="3" class="muted">No files yet. Start a new design or upload one.</td></tr>`;
  $("#files").querySelectorAll("[data-detail]").forEach((b) => b.onclick = () => openDetail(b.dataset.detail));
  $("#files").querySelectorAll("[data-force]").forEach((b) => b.onclick = async () => {
    const holder = lockOf(b.dataset.force);
    if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = `Sure? ${holder ? holder.user_name + " may lose unsaved work" : ""}`; return; }
    try { await api("POST", `/api/projects/${PID}/lock/force`, { path: b.dataset.force }); } catch (e) { showMsg($("#msg"), e.message, true); }
  });
}

async function openDetail(path) {
  OPEN = path; $("#detail").hidden = false; $("#d-title").textContent = path;
  const [h, c] = await Promise.all([api("GET", `/api/projects/${PID}/history?path=${encodeURIComponent(path)}`), api("GET", `/api/projects/${PID}/comments?path=${encodeURIComponent(path)}`)]);
  $("#d-versions").innerHTML = h.versions.map((v, i) => `<tr><td><span class="mono">${esc(v.id)}</span></td><td>${esc(v.author)}</td><td>${ago(v.at)}</td><td>${esc(v.message)}</td>
    <td>${i > 0 && INFO.can_edit ? `<button data-restore="${esc(v.id)}">Restore</button>` : i === 0 ? `<span class="tag">current</span>` : ""}</td></tr>`).join("");
  $("#d-versions").querySelectorAll("[data-restore]").forEach((b) => b.onclick = async () => {
    try { await api("POST", `/api/projects/${PID}/restore`, { path, version: b.dataset.restore }); openDetail(path); } catch (e) { showMsg($("#msg"), e.message, true); }
  });
  $("#d-comments").innerHTML = c.comments.map((x) => `<div class="item">${avatar({ name: x.user_name, color: x.color })}<div><strong>${esc(x.user_name)}</strong> <span class="muted">${ago(x.created_at)}</span><br>${esc(x.body)}</div></div>`).join("") || `<span class="muted">No comments yet.</span>`;
  $("#detail").scrollIntoView({ behavior: "smooth" });
}
$("#d-close").onclick = () => { $("#detail").hidden = true; OPEN = null; };
$("#d-form").onsubmit = async (e) => {
  e.preventDefault();
  try { await api("POST", `/api/projects/${PID}/comments`, { path: OPEN, body: $("#d-body").value }); $("#d-body").value = ""; openDetail(OPEN); } catch (err) { showMsg($("#msg"), err.message, true); }
};

$("#upload").onchange = async (e) => {
  for (const file of e.target.files) {
    const exists = INFO.files.some((f) => f.path === file.name);
    if (exists) { showMsg($("#msg"), `${file.name} already exists. Open it for editing to replace it, or rename the file first.`, true); continue; }
    showMsg($("#msg"), `Uploading ${file.name}…`);
    try {
      const r = await api("PUT", `/api/projects/${PID}/files/${encodeURIComponent(file.name)}?message=${encodeURIComponent("Uploaded " + file.name)}`, await file.arrayBuffer(), true);
      showMsg($("#msg"), r.converting ? `Uploaded ${file.name}. Its design is being made and will appear in the list when ready.`
        : r.queue_full ? `Uploaded ${file.name}. The server is busy making other designs; upload it again later to get a design.` : `Uploaded ${file.name}.`);
    } catch (err) { showMsg($("#msg"), `${file.name}: ${err.message}`, true); }
  }
  e.target.value = ""; refresh();
};
$("#new-design").onclick = () => {
  let name = prompt("Name for the new design");
  if (!name) return;
  name = name.trim().replace(/\.json$/i, "") + ".json";
  if (INFO.files.some((f) => f.path === name)) { showMsg($("#msg"), `${name} already exists.`, true); return; }
  location.href = `/edit.html?p=${PID}&f=${encodeURIComponent(name)}&new=1`;
};

function renderMembers() {
  $("#members").innerHTML = INFO.members.map((m) => chip(m, m.name + (m.id === INFO.project.owner_id ? " (owner)" : ""))).join("");
}

async function refresh() {
  INFO = await api("GET", `/api/projects/${PID}`);
  const cfg = await api("GET", "/api/config");
  INFO.editor = cfg.editor;
  $("#cad-note").hidden = !cfg.cad_import;
  $("#title").textContent = INFO.project.name;
  document.title = `${INFO.project.name} · ${cfg.site_name}`;
  $("#sub").textContent = `${INFO.team ? INFO.team.name + " · " : ""}${INFO.project.kind === "joint" ? "Joint project" : "Personal project"}`;
  $("#actions").hidden = !INFO.can_edit;
  LOCKS = INFO.locks; PEOPLE = INFO.people;
  renderFiles(); renderHere(); renderMembers();
  if (INFO.can_manage) {
    const team = await api("GET", `/api/teams/${INFO.project.team_id}/members`);
    const others = team.members.filter((m) => !INFO.members.some((x) => x.id === m.id) && m.status === "active");
    $("#share").hidden = !others.length;
    $("#share-who").innerHTML = others.map((m) => `<option value="${m.id}">${esc(m.name)} (${esc(m.role)})</option>`).join("");
  }
}
$("#share").onsubmit = async (e) => {
  e.preventDefault();
  try { await api("POST", `/api/projects/${PID}/members`, { user_id: Number($("#share-who").value) }); refresh(); } catch (err) { showMsg($("#msg"), err.message, true); }
};

header("projects").then(async () => {
  await refresh();
  const live = liveChannel(PID, (m) => {
    if (m.type === "hello" || m.type === "presence") { PEOPLE = m.people; renderHere(); renderFiles(); }
    if (m.type === "hello" || m.type === "locks") { LOCKS = m.locks; renderFiles(); if (m.forced) feed(`<span><strong>${esc(m.forced.by)}</strong> freed ${esc(m.forced.path)}</span>`); }
    if (m.type === "saved") { feed(`${avatar(m.by)}<span>${m.made_from ? `Design <strong>${esc(m.path)}</strong> is ready (made from ${esc(m.made_from)})` : `<strong>${esc(m.by.name)}</strong> saved ${esc(m.path)}`} <span class="mono">${esc(m.version)}</span></span>`); refresh(); if (OPEN === m.path) openDetail(OPEN); }
    if (m.type === "convert_failed") { feed(`${avatar(m.by)}<span>No design could be made from <strong>${esc(m.path)}</strong>; the file itself is saved.</span>`); }
    if (m.type === "comment") { feed(`${avatar(m.by)}<span><strong>${esc(m.by.name)}</strong> on ${esc(m.path)}: ${esc(m.body)}</span>`); if (OPEN === m.path) openDetail(OPEN); }
  });
  live.send({ type: "presence", mode: "browsing" });
}).catch((e) => showMsg($("#msg"), e.message, true));
