let ME;
async function load() {
  const { teams } = await api("GET", "/api/teams");
  const box = $("#teams");
  if (!teams.length) { box.innerHTML = `<div class="panel"><h2>No team yet</h2><p class="muted">Your account is approved but not in a team. Ask your admin to add you to one.</p></div>`; return; }
  box.innerHTML = "";
  for (const t of teams) {
    const { projects } = await api("GET", `/api/teams/${t.id}/projects`);
    const sec = document.createElement("section");
    sec.className = "panel";
    const canCreate = t.role !== "viewer";
    sec.innerHTML = `<div class="row spread"><h1>${esc(t.name)}</h1><span class="tag">${esc(t.role)}</span></div>
      <div class="cards">${projects.map(card).join("") || `<p class="muted">No projects yet.</p>`}</div>
      ${canCreate ? `<form class="row" data-team="${t.id}"><input name="name" placeholder="New project name" required maxlength="80">
        <select name="kind"><option value="joint">Joint (whole team)</option><option value="personal">Personal (only me)</option></select>
        <button class="primary">Create project</button></form>` : ""}`;
    box.append(sec);
  }
  box.querySelectorAll("form[data-team]").forEach((f) => f.onsubmit = async (e) => {
    e.preventDefault();
    try {
      const r = await api("POST", `/api/teams/${f.dataset.team}/projects`, { name: f.name.value, kind: f.kind.value });
      location.href = `/project.html?id=${r.id}`;
    } catch (err) { showMsg($("#msg"), err.message, true); }
  });
}
function card(row) {
  const p = row.project;
  const editing = row.locks.map((l) => `${avatar({ name: l.user_name, color: l.color }, l.user_name + " is editing " + l.path)}`).join("");
  const here = row.people.filter((x, i, a) => a.findIndex((y) => y.user_id === x.user_id) === i);
  return `<a class="card" href="/project.html?id=${p.id}">
    <div class="row spread"><strong>${esc(p.name)}</strong><span class="tag">${p.kind === "joint" ? "Joint" : "Personal"}</span></div>
    <span class="muted">Owner: ${esc(row.owner_name)}</span>
    <div class="row">${here.length ? `<span class="avatars">${here.map((x) => avatar(x, x.name + " is here")).join("")}</span><span class="muted">${here.length} here now</span>` : `<span class="muted">Nobody here now</span>`}</div>
    ${row.locks.length ? `<div class="row"><span class="avatars">${editing}</span><span class="muted">${row.locks.length} part${row.locks.length > 1 ? "s" : ""} being edited</span></div>` : ""}
  </a>`;
}
header("projects").then(({ me }) => { ME = me; load(); setInterval(load, 15000); }).catch((e) => showMsg($("#msg"), e.message, true));
