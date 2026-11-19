let TEAMS = [], ME;
const roleSelect = (id, sel) => `<select id="${id}"><option value="editor" ${sel === "editor" ? "selected" : ""}>Editor</option><option value="viewer" ${sel === "viewer" ? "selected" : ""}>Viewer</option><option value="owner" ${sel === "owner" ? "selected" : ""}>Owner</option></select>`;
const teamSelect = (id) => `<select id="${id}">${TEAMS.map((t) => `<option value="${t.id}">${esc(t.name)}</option>`).join("")}</select>`;

async function load() {
  TEAMS = (await api("GET", "/api/admin/teams")).teams;
  const pend = (await api("GET", "/api/admin/users?status=pending")).users;
  $("#pending").innerHTML = pend.map(({ user: u }) => `<tr><td>${avatar(u)} <strong>${esc(u.name)}</strong><br><span class="muted">${esc(u.email)}</span></td>
    <td>${ago(u.created_at)}</td><td>${TEAMS.length ? teamSelect("t" + u.id) : `<span class="muted">Create a team first</span>`}</td><td>${roleSelect("r" + u.id, "editor")}</td>
    <td><div class="row"><button class="primary" data-approve="${u.id}" ${TEAMS.length ? "" : "disabled"}>Approve</button><button class="danger" data-reject="${u.id}">Reject</button></div></td></tr>`).join("")
    || `<tr><td colspan="5" class="muted">Nobody is waiting.</td></tr>`;

  const all = (await api("GET", "/api/admin/users")).users;
  $("#users").innerHTML = all.filter((r) => r.user.status !== "pending").map(({ user: u, teams }) => `<tr>
    <td>${avatar(u)} <strong>${esc(u.name)}</strong>${u.is_admin ? ' <span class="tag">admin</span>' : ""}<br><span class="muted">${esc(u.email)}</span></td>
    <td>${u.status === "active" ? "Active" : "Switched off"}</td>
    <td>${teams.map((t) => `${esc(t.name)} <span class="tag">${esc(t.role)}</span>`).join("<br>") || '<span class="muted">none</span>'}</td>
    <td><div class="row">
      <button data-reset="${u.id}">Reset password</button>
      ${u.id !== ME.id ? (u.status === "active" ? `<button class="danger" data-disable="${u.id}">Switch off</button>` : `<button data-enable="${u.id}">Switch on</button>`) : ""}
      ${u.id !== ME.id ? `<button data-admin="${u.id}" data-to="${!u.is_admin}">${u.is_admin ? "Remove admin" : "Make admin"}</button>` : ""}
    </div></td></tr>`).join("");

  const box = $("#teams"); box.innerHTML = "";
  for (const t of TEAMS) {
    const m = (await api("GET", `/api/teams/${t.id}/members`)).members;
    const outside = all.filter((r) => r.user.status === "active" && !m.some((x) => x.id === r.user.id));
    const d = document.createElement("div"); d.className = "panel";
    d.innerHTML = `<div class="row spread"><strong>${esc(t.name)}</strong><span class="muted">${m.length} member${m.length === 1 ? "" : "s"}</span></div>
      <div class="table-wrap"><table><tbody>${m.map((x) => `<tr><td>${avatar(x)} ${esc(x.name)} <span class="muted">${esc(x.email)}</span></td>
        <td>${roleSelect(`m${t.id}-${x.id}`, x.role)}</td><td><div class="row"><button data-role="${t.id}:${x.id}">Change role</button><button class="danger" data-remove="${t.id}:${x.id}">Remove from team</button></div></td></tr>`).join("")}</tbody></table></div>
      ${outside.length ? `<form class="row" data-add="${t.id}"><select name="who">${outside.map((r) => `<option value="${r.user.id}">${esc(r.user.name)} (${esc(r.user.email)})</option>`).join("")}</select>${roleSelect(`add${t.id}`, "editor")}<button>Add to team</button></form>` : ""}`;
    box.append(d);
  }
  wire();
}

function act(fn) { return async (e) => { try { await fn(e.currentTarget); await load(); } catch (err) { showMsg($("#msg"), err.message, true); } }; }
function wire() {
  document.querySelectorAll("[data-approve]").forEach((b) => b.onclick = act(async (b) => {
    const id = b.dataset.approve;
    await api("POST", `/api/admin/users/${id}/approve`, { team_id: Number($("#t" + id).value), role: $("#r" + id).value });
    showMsg($("#msg"), "Approved. They can sign in now.");
  }));
  document.querySelectorAll("[data-reject]").forEach((b) => b.onclick = act(async (b) => { await api("POST", `/api/admin/users/${b.dataset.reject}/reject`); showMsg($("#msg"), "Request rejected and removed."); }));
  document.querySelectorAll("[data-disable]").forEach((b) => b.onclick = act(async (b) => { await api("POST", `/api/admin/users/${b.dataset.disable}/disable`); }));
  document.querySelectorAll("[data-enable]").forEach((b) => b.onclick = act(async (b) => { await api("POST", `/api/admin/users/${b.dataset.enable}/enable`); }));
  document.querySelectorAll("[data-admin]").forEach((b) => b.onclick = act(async (b) => { await api("POST", `/api/admin/users/${b.dataset.admin}/admin`, { admin: b.dataset.to === "true" }); }));
  document.querySelectorAll("[data-reset]").forEach((b) => b.onclick = async () => {
    try { const r = await api("POST", `/api/admin/users/${b.dataset.reset}/reset-password`); showMsg($("#msg"), `One-time password: ${r.temporary_password} — give it to them privately; they'll be asked to change it.`); }
    catch (err) { showMsg($("#msg"), err.message, true); }
  });
  document.querySelectorAll("[data-role]").forEach((b) => b.onclick = act(async (b) => {
    const [t, u] = b.dataset.role.split(":"); await api("POST", `/api/teams/${t}/members`, { user_id: Number(u), role: $(`#m${t}-${u}`).value });
  }));
  document.querySelectorAll("[data-remove]").forEach((b) => b.onclick = act(async (b) => { const [t, u] = b.dataset.remove.split(":"); await api("DELETE", `/api/teams/${t}/members/${u}`); }));
  document.querySelectorAll("form[data-add]").forEach((f) => f.onsubmit = (e) => { e.preventDefault(); act(async () => {
    await api("POST", `/api/teams/${f.dataset.add}/members`, { user_id: Number(f.who.value), role: $(`#add${f.dataset.add}`).value });
  })({ currentTarget: f }); });
}
$("#new-team").onsubmit = (e) => { e.preventDefault(); act(async () => { await api("POST", "/api/admin/teams", { name: $("#team-name").value }); $("#team-name").value = ""; })({ currentTarget: e.target }); };
header("admin").then(({ me }) => { ME = me.user; if (!ME.is_admin) { showMsg($("#msg"), "Only admins can open this page.", true); return; } load(); }).catch((e) => showMsg($("#msg"), e.message, true));
