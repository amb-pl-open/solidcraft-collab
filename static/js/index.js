// Only ever go to a page on this site after signing in (never javascript: or another host).
function safeNext() {
  const n = qs("next");
  if (!n) return "/app.html";
  try {
    const u = new URL(n, location.origin);
    if (u.origin !== location.origin || !u.pathname.startsWith("/")) return "/app.html";
    return u.pathname + u.search;
  } catch { return "/app.html"; }
}
api("GET", "/api/config").then((c) => { $("#site").textContent = c.site_name; document.title = "Sign in · " + c.site_name; });
api("GET", "/api/me").then(() => { location.href = safeNext(); }).catch(() => {});
$("#show-signup").onclick = (e) => { e.preventDefault(); $("#login").hidden = true; $("#signup").hidden = false; showMsg($("#msg"), ""); };
$("#show-login").onclick = (e) => { e.preventDefault(); $("#login").hidden = false; $("#signup").hidden = true; showMsg($("#msg"), ""); };
$("#login").onsubmit = async (e) => {
  e.preventDefault();
  try {
    const r = await api("POST", "/api/auth/login", { email: $("#email").value, password: $("#password").value });
    location.href = r.user.must_change_pw ? "/account.html?change=1" : safeNext();
  } catch (err) { showMsg($("#msg"), err.message, true); }
};
$("#signup").onsubmit = async (e) => {
  e.preventDefault();
  try {
    const r = await api("POST", "/api/auth/signup", { name: $("#s-name").value, email: $("#s-email").value, password: $("#s-password").value });
    showMsg($("#msg"), r.message); $("#signup").hidden = true; $("#login").hidden = false;
  } catch (err) { showMsg($("#msg"), err.message, true); }
};
