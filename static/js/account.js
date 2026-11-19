header("account").then(({ me }) => {
  $("#who").innerHTML = `${avatar(me.user)}<div><strong>${esc(me.user.name)}</strong><br><span class="muted">${esc(me.user.email)}</span></div>`;
  $("#teams").textContent = me.teams.length ? "Teams: " + me.teams.map((t) => `${t.name} (${t.role})`).join(", ") : "Not in a team yet.";
  if (qs("change") || me.user.must_change_pw) showMsg($("#msg"), "You signed in with a one-time password. Choose your own password now.");
}).catch((e) => showMsg($("#msg"), e.message, true));
$("#pw").onsubmit = async (e) => {
  e.preventDefault();
  try { const r = await api("POST", "/api/auth/password", { current: $("#cur").value, new: $("#new").value }); showMsg($("#msg"), r.message); setTimeout(() => location.href = "/", 1500); }
  catch (err) { showMsg($("#msg"), err.message, true); }
};
