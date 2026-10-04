"use strict";
/* EnigmaSaurus frontend: vanilla JS over the Tauri command bridge. */

const T = window.__TAURI__;
if (!T) {
  document.body.innerHTML = "<p style='padding:2em'>This page must run inside the EnigmaSaurus app.</p>";
  throw new Error("no Tauri bridge");
}
const invoke = T.core.invoke;
const listen = T.event.listen;

const $ = (id) => document.getElementById(id);
const letter = (v) => String.fromCharCode(65 + v);
const showError = (id, msg) => {
  const el = $(id);
  if (!msg) { el.hidden = true; el.textContent = ""; return; }
  el.hidden = false; el.textContent = msg;
};

/* ---------- theme: jungle night / savanna day ---------- */
function applyTheme(theme) {
  document.documentElement.dataset.theme = theme === "light" ? "light" : "";
  $("theme-toggle").textContent = theme === "light" ? "☀️" : "🌙";
  try { localStorage.setItem("enigmasaurus-theme", theme); } catch {}
}
applyTheme((() => { try { return localStorage.getItem("enigmasaurus-theme"); } catch { return null; } })() || "");
$("theme-toggle").addEventListener("click", () => {
  const next = document.documentElement.dataset.theme === "light" ? "" : "light";
  applyTheme(next);
});

/* ---------- first-run tour ---------- */
const TOUR = [
  ["Welcome to the nest", "This is a real Enigma machine. Open the Machine tab, press Load machine, and type AAAAA. History says the answer is BDZGO."],
  ["Hunt with a clue", "The Crib hunt tab breaks messages when you can guess a word inside. Press Fill demo, then Start hunt, and watch it recover the full settings."],
  ["Hunt with nothing", "The Blind hunt tab needs no guesses at all, just a long message. Fill demo again, start it, and it finds rotors, positions, and plugs."],
];
(function tour() {
  let seen = false;
  try { seen = localStorage.getItem("enigmasaurus-tour") === "done"; } catch {}
  if (seen) return;
  let step = 0;
  const box = $("onboarding");
  const render = () => {
    $("ob-title").textContent = `Step ${step + 1} of ${TOUR.length}: ${TOUR[step][0]}`;
    $("ob-body").textContent = TOUR[step][1];
    $("ob-next").textContent = step + 1 === TOUR.length ? "Start digging" : "Next";
  };
  const finish = (completed) => {
    try {
      if ($("ob-hide").checked || completed) {
        localStorage.setItem("enigmasaurus-tour", "done");
      }
    } catch {}
    box.hidden = true;
  };
  $("ob-next").addEventListener("click", () => {
    if (step + 1 === TOUR.length) { finish(true); return; }
    step++;
    render();
  });
  $("ob-skip").addEventListener("click", () => finish(false));
  box.hidden = false;
  render();
})();

/* ---------- tabs ---------- */
document.querySelectorAll(".tab").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tabpane").forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    $("tab-" + btn.dataset.tab).classList.add("active");
  });
});

/* ---------- machine tab ---------- */
const ROTORS = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII"];
const FOURTHS = ["None", "Beta", "Gamma"];
function fillSelect(id, choices, initial) {
  const el = $(id);
  el.innerHTML = "";
  for (const c of choices) {
    const o = document.createElement("option");
    o.value = c; o.textContent = c;
    if (c === initial) o.selected = true;
    el.appendChild(o);
  }
}
fillSelect("m-r1", ROTORS, "I");
fillSelect("m-r2", ROTORS, "II");
fillSelect("m-r3", ROTORS, "III");
fillSelect("m-r4", FOURTHS, "None");

let machineLoaded = false;
function machineArgs() {
  const rotors = [$("m-r1").value, $("m-r2").value, $("m-r3").value];
  if ($("m-r4").value !== "None") rotors.push($("m-r4").value);
  return {
    rotors,
    rings: $("m-rings").value,
    positions: $("m-pos").value,
    reflector: $("m-ref").value,
    plugs: $("m-plugs").value,
    etw: $("m-etw").value,
  };
}
function applySection(s) {
  const names = s.rotors || [];
  const picks = [$("m-r1"), $("m-r2"), $("m-r3")];
  picks.forEach((el, i) => { if (names[i]) el.value = names[i]; });
  const fourth = names.length > 3 ? names[3] : "None";
  if ([...$("m-r4").options].some((o) => o.value === fourth)) $("m-r4").value = fourth;
  if (s.rings) $("m-rings").value = s.rings;
  if (s.positions) $("m-pos").value = s.positions;
  if (s.reflector) $("m-ref").value = s.reflector;
  if (s.plugs !== undefined && s.plugs !== null) $("m-plugs").value = s.plugs;
  if (s.etw) $("m-etw").value = s.etw;
}
async function loadMachine() {
  showError("m-error", "");
  try {
    const summary = await invoke("machine_load", { args: machineArgs() });
    $("m-summary").textContent = summary;
    $("m-welcome").hidden = true;
    $("m-live").hidden = false;
    machineLoaded = true;
    await retype();
  } catch (e) { showError("m-error", String(e)); }
}
function formatTrace(t) {
  const parts = [`IN:${letter(t.input)}`, `STB:${letter(t.plugIn)}`, `ETW:${letter(t.etwIn)}`];
  for (let i = 0; i < t.rotorCount; i++) parts.push(`R${t.rotorCount - i}:${letter(t.rotorFwd[i])}`);
  parts.push(`UKW:${letter(t.reflected)}`);
  for (let i = 0; i < t.rotorCount; i++) parts.push(`R${i + 1}:${letter(t.rotorBwd[i])}`);
  parts.push(`ETW:${letter(t.etwOut)}`, `STB:${letter(t.output)}`);
  return parts.join(" ");
}
async function retype() {
  if (!machineLoaded) return;
  try {
    const r = await invoke("machine_type", { full_text: $("m-input").value });
    $("m-output").textContent = r.output;
    const win = $("m-windows");
    const next = [...r.windows].join(" ");
    if (win.textContent !== next) {
      win.textContent = next;
      win.classList.remove("bump");
      void win.offsetWidth;
      win.classList.add("bump");
    }
    $("m-trace").textContent = r.trace ? formatTrace(r.trace) : "";
  } catch (e) { showError("m-error", String(e)); }
}
$("m-load").addEventListener("click", loadMachine);
$("m-dice").addEventListener("click", () => {
  const n = $("m-r4").value === "None" ? 3 : 4;
  $("m-pos").value = Array.from({ length: n }, () =>
    String.fromCharCode(65 + Math.floor(Math.random() * 26))).join("");
});
$("m-input").addEventListener("input", retype);
$("m-clear").addEventListener("click", async () => {
  $("m-input").value = "";
  try {
    const w = await invoke("machine_clear");
    $("m-windows").textContent = [...w].join(" ");
    $("m-output").innerHTML = '<span class="dim">Encrypted text appears here as you type.</span>';
    $("m-trace").textContent = "";
  } catch (e) { showError("m-error", String(e)); }
});
$("m-copy").addEventListener("click", () => {
  navigator.clipboard.writeText($("m-output").textContent).catch(() => {});
});
$("m-save").addEventListener("click", async () => {
  const path = await invoke("dialog_save_txt", { name: "message.txt" });
  if (path) await invoke("text_write", { path, contents: $("m-output").textContent });
});
$("m-loadcfg").addEventListener("click", async () => {
  const path = await invoke("dialog_open_toml");
  if (!path) return;
  try {
    applySection(await invoke("config_read", { path }));
    await loadMachine();
  } catch (e) { showError("m-error", String(e)); }
});
$("m-savecfg").addEventListener("click", async () => {
  const path = await invoke("dialog_save_toml");
  if (!path) return;
  try {
    await invoke("config_write", { path, section: machineArgs() });
  } catch (e) { showError("m-error", String(e)); }
});

/* ---------- crib tab ---------- */
let cribPoll = null, cribWinners = [], cribPreviewFull = "", cribCribLen = 0;
function cribRow(c, i) {
  return `<tr class="new-row" data-i="${i}"><td>#${i + 1}</td><td>${c.order.join(" ")}</td>` +
    `<td>${c.positions.map(letter).join("")}</td><td>${c.matches}</td><td>${c.score.toFixed(1)}</td></tr>`;
}
async function cribRefresh() {
  try {
    cribWinners = await invoke("crib_live");
    $("c-empty").hidden = cribWinners.length > 0;
    const tb = $("c-table").querySelector("tbody");
    tb.innerHTML = cribWinners.map(cribRow).join("");
    tb.querySelectorAll("tr").forEach((tr) => {
      tr.addEventListener("click", async () => {
        tb.querySelectorAll("tr").forEach((r) => r.classList.remove("sel"));
        tr.classList.add("sel");
        const c = cribWinners[Number(tr.dataset.i)];
        cribPreviewFull = await invoke("crib_preview", { order: c.order, positions: c.positions });
        $("c-preview").textContent = cribPreviewFull.slice(0, 600);
      });
    });
  } catch (e) { showError("c-error", String(e)); }
}
$("c-demo").addEventListener("click", async () => {
  try {
    $("c-cipher").value = (await invoke("demo_text", { name: "crib_cipher.txt" })).trim();
  } catch { showError("c-error", "Demo file not found next to the app."); return; }
  $("c-crib").value = "MORGENGRAUEN";
  $("c-pool").value = "I II III";
  $("c-lang").value = "de";
  showError("c-error", "");
});
$("c-start").addEventListener("click", async () => {
  showError("c-error", "");
  if (!$("c-cipher").value.trim() || !$("c-crib").value.trim()) {
    showError("c-error", "Paste a ciphertext and type the guessed word first, or press “Fill demo”.");
    return;
  }
  const params = {
    pool: $("c-pool").value.split(/\s+/).filter(Boolean),
    fourth: $("c-fourth").value || null,
    rings: $("c-rings").value, reflector: $("c-ref").value, plugs: $("c-plugs").value,
    etw: $("c-etw").value, lang: $("c-lang").value,
    cipher: $("c-cipher").value, crib: $("c-crib").value,
    offset: $("c-offset").value.trim() === "" ? null : Number($("c-offset").value),
    top: Number($("c-top").value) || 5,
    ringScan: $("c-ringscan").value.split(",").map((s) => Number(s.trim())).filter((n) => Number.isInteger(n)),
    inferPlugs: $("c-infer").checked,
  };
  try {
    await invoke("crib_start", { params });
    $("c-status").textContent = "Hunting…";
    $("c-empty").hidden = true;
    cribWinners = []; cribPreviewFull = ""; $("c-preview").textContent = "";
    clearInterval(cribPoll);
    cribPoll = setInterval(cribRefresh, 600);
  } catch (e) { showError("c-error", String(e)); }
});
$("c-save").addEventListener("click", async () => {
  if (!cribPreviewFull) return;
  const path = await invoke("dialog_save_txt", { name: "crib_result.txt" });
  if (path) await invoke("text_write", { path, contents: cribPreviewFull });
});
$("c-copy").addEventListener("click", () => {
  if (cribPreviewFull) navigator.clipboard.writeText(cribPreviewFull).catch(() => {});
});
document.querySelector("#tab-crib").addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") $("c-start").click();
});
$("c-loadcfg").addEventListener("click", async () => {
  const path = await invoke("dialog_open_toml");
  if (!path) return;
  try {
    const s = await invoke("config_read", { path });
    const names = s.rotors || [];
    if (names[0] === "Beta" || names[0] === "Gamma") {
      $("c-fourth").value = names[0];
      $("c-pool").value = names.slice(1).join(" ");
    } else {
      $("c-fourth").value = "";
      $("c-pool").value = names.join(" ");
    }
    if (s.rings) $("c-rings").value = s.rings;
    if (s.reflector) $("c-ref").value = s.reflector;
    if (s.plugs !== undefined && s.plugs !== null) $("c-plugs").value = s.plugs;
    if (s.etw) $("c-etw").value = s.etw;
    const sv = await invoke("solver_read", { path });
    if (sv.lang) $("c-lang").value = sv.lang;
    if (sv.top !== undefined && sv.top !== null) $("c-top").value = String(sv.top);
  } catch (e) { showError("c-error", String(e)); }
});
await listen("crib-progress", (e) => {
  const { done, total } = e.payload;
  $("c-bar").value = (done / Math.max(1, total)) * 100;
  $("c-status").textContent = `Trying rotor orders… ${done} of ${total}`;
});
await listen("crib-done", async () => {
  clearInterval(cribPoll);
  $("c-bar").value = 100;
  $("c-status").textContent = "Done.";
  await cribRefresh();
});
await listen("crib-error", (e) => {
  clearInterval(cribPoll);
  showError("c-error", String(e.payload));
});

/* ---------- blind tab ---------- */
let blindWinners = [], blindPreviewFull = "";
function blindRow(c, i) {
  const plugs = c.plugs.length
    ? c.plugs.map(([a, b]) => letter(a) + letter(b)).join(" ") : "None";
  return `<tr class="new-row" data-i="${i}"><td>#${i + 1}</td><td>${c.order.join(" ")}</td>` +
    `<td>${c.positions.map(letter).join("")}</td><td>${plugs}</td><td>${c.score.toFixed(1)}</td></tr>`;
}
$("b-demo").addEventListener("click", async () => {
  try {
    $("b-cipher").value = (await invoke("demo_text", { name: "blind_cipher.txt" })).trim();
  } catch { showError("b-error", "Demo file not found next to the app."); return; }
  $("b-pool").value = "I II III";
  $("b-lang").value = "en";
  $("b-maxplugs").value = "6";
  $("b-toppos").value = "8";
  $("b-restarts").value = "3";
  showError("b-error", "");
});
$("b-start").addEventListener("click", async () => {
  showError("b-error", "");
  if (!$("b-cipher").value.trim()) {
    showError("b-error", "Paste a ciphertext first, or press “Fill demo”.");
    return;
  }
  const letters = $("b-cipher").value.replace(/[^a-zA-Z]/g, "").length;
  $("b-warn").hidden = letters >= 100;
  if (letters < 100) {
    $("b-warn").textContent =
      `Only ${letters} letters: short tracks overfit. Try max plugs 0 first, and check the language.`;
  }
  const params = {
    pool: $("b-pool").value.split(/\s+/).filter(Boolean),
    fourth: $("b-fourth").value || null,
    rings: $("b-rings").value, reflector: $("b-ref").value, etw: $("b-etw").value,
    lang: $("b-lang").value, cipher: $("b-cipher").value,
    maxPlugs: Number($("b-maxplugs").value) || 0,
    topPositions: Number($("b-toppos").value) || 10,
    restarts: Number($("b-restarts").value) || 0,
    seed: Number($("b-seed").value) || 1,
    top: Number($("b-top").value) || 3,
    ringScan: $("b-ringscan").value.split(",").map((s) => Number(s.trim())).filter((n) => Number.isInteger(n)),
  };
  try {
    await invoke("blind_start", { params });
    blindWinners = []; blindPreviewFull = "";
    $("b-empty").hidden = true;
    $("b-table").querySelector("tbody").innerHTML = "";
    $("b-preview").textContent = "";
  } catch (e) { showError("b-error", String(e)); }
});
$("b-save").addEventListener("click", async () => {
  if (!blindPreviewFull) return;
  const path = await invoke("dialog_save_txt", { name: "blind_result.txt" });
  if (path) await invoke("text_write", { path, contents: blindPreviewFull });
});
$("b-copy").addEventListener("click", () => {
  if (blindPreviewFull) navigator.clipboard.writeText(blindPreviewFull).catch(() => {});
});
document.querySelector("#tab-blind").addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") $("b-start").click();
});
$("b-loadcfg").addEventListener("click", async () => {
  const path = await invoke("dialog_open_toml");
  if (!path) return;
  try {
    const s = await invoke("config_read", { path });
    const names = s.rotors || [];
    if (names[0] === "Beta" || names[0] === "Gamma") {
      $("b-fourth").value = names[0];
      $("b-pool").value = names.slice(1).join(" ");
    } else {
      $("b-fourth").value = "";
      $("b-pool").value = names.join(" ");
    }
    if (s.rings) $("b-rings").value = s.rings;
    if (s.reflector) $("b-ref").value = s.reflector;
    if (s.etw) $("b-etw").value = s.etw;
    const sv = await invoke("solver_read", { path });
    if (sv.lang) $("b-lang").value = sv.lang;
    if (sv.maxPlugs !== undefined && sv.maxPlugs !== null) $("b-maxplugs").value = String(sv.maxPlugs);
    if (sv.topPositions !== undefined && sv.topPositions !== null) $("b-toppos").value = String(sv.topPositions);
    if (sv.restarts !== undefined && sv.restarts !== null) $("b-restarts").value = String(sv.restarts);
    if (sv.seed !== undefined && sv.seed !== null) $("b-seed").value = String(sv.seed);
    if (sv.top !== undefined && sv.top !== null) $("b-top").value = String(sv.top);
  } catch (e) { showError("b-error", String(e)); }
});
function setBar(id, tid, done, total) {
  $(id).value = (done / Math.max(1, total)) * 100;
  $(tid).textContent = `${done}/${total}`;
}
await listen("blind-progress", (e) => {
  const { stage, done, total } = e.payload;
  if (stage === "scan") setBar("b-scan", "b-scant", done, total);
  else if (stage === "climb") setBar("b-climb", "b-climbt", done, total);
  else setBar("b-ord", "b-ordt", done, total);
});
await listen("blind-done", (e) => {
  blindWinners = e.payload;
  $("b-empty").hidden = blindWinners.length > 0;
  const tb = $("b-table").querySelector("tbody");
  tb.innerHTML = blindWinners.map(blindRow).join("");
  tb.querySelectorAll("tr").forEach((tr) => {
    tr.addEventListener("click", () => {
      tb.querySelectorAll("tr").forEach((r) => r.classList.remove("sel"));
      tr.classList.add("sel");
      const c = blindWinners[Number(tr.dataset.i)];
      blindPreviewFull = c.plaintext;
      $("b-preview").textContent = c.plaintext.slice(0, 600);
    });
  });
  if (blindWinners.length) {
    tb.querySelector("tr").classList.add("sel");
    blindPreviewFull = blindWinners[0].plaintext;
    $("b-preview").textContent = blindPreviewFull.slice(0, 600);
  }
});
await listen("blind-error", (e) => {
  showError("b-error", String(e.payload));
});
