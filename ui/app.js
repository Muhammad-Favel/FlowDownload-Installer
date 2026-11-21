const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;

const TRADEOFFS = {
  full: "Uses all 3 App IDs (FlowDownload + Safari + Share extensions). Cannot coexist with other sideloaded apps on a free Apple ID.",
  "share-only": "Uses 2 App IDs. Keeps Share Sheet handoff from Safari/Files; drops the Safari toolbar popup.",
  core: "Uses 1 App ID. Keeps Boost, libtorrent, built-in AdGuard browser, USB export, and SHA-256. Drops Safari & Share extensions."
};

const PROFILE_IDS = { full: 3, "share-only": 2, core: 1 };

let selectedProfile = "full";

function selectProfile(profile) {
  selectedProfile = profile;
  const count = PROFILE_IDS[profile] || 3;
  document.querySelectorAll(".profile-row").forEach((btn) => {
    btn.classList.toggle("is-selected", btn.dataset.profile === profile);
  });
  document.getElementById("budgetCount").textContent = `${count} / 3 IDs`;
  [1, 2, 3].forEach((idx) => {
    document.getElementById(`slot${idx}`).classList.toggle("is-filled", idx <= count);
  });
  document.getElementById("tradeoffBox").textContent = TRADEOFFS[profile];
}

document.querySelectorAll(".profile-row").forEach((btn) => {
  btn.addEventListener("click", () => selectProfile(btn.dataset.profile));
});
selectProfile("full");

async function refreshRelease() {
  const badge = document.getElementById("releaseBadge");
  if (!invoke) {
    badge.textContent = "Preview mode";
    return;
  }
  try {
    const info = await invoke("get_latest_release");
    badge.textContent = `v${info.version} · ${(info.size / 1_000_000).toFixed(1)} MB`;
  } catch (err) {
    badge.textContent = "Offline";
  }
}

async function refreshDevices() {
  const dot = document.getElementById("deviceDot");
  const title = document.getElementById("deviceTitle");
  const sub = document.getElementById("deviceSub");
  if (!invoke) return;

  try {
    const devices = await invoke("list_usb_devices");
    if (devices.length > 0) {
      dot.classList.add("online");
      title.textContent = `${devices[0].name} connected`;
      sub.textContent = `UDID: ${devices[0].udid}`;
    } else {
      dot.classList.remove("online");
      title.textContent = "No iPhone detected";
      sub.textContent = "Connect your iPhone via USB and tap Trust This Computer";
    }
  } catch (err) {
    dot.classList.remove("online");
    title.textContent = "USB service unavailable";
    sub.textContent = String(err);
  }
}

document.getElementById("refreshDevicesBtn").addEventListener("click", refreshDevices);

function appendLog(line) {
  const body = document.getElementById("logsBody");
  if (!body) return;
  if (body.textContent === "Ready.") body.textContent = "";
  body.textContent += (body.textContent ? "\n" : "") + line;
  body.scrollTop = body.scrollHeight;
}

document.getElementById("copyLogsBtn").addEventListener("click", async (e) => {
  e.preventDefault();
  e.stopPropagation();
  const body = document.getElementById("logsBody");
  if (!body) return;
  try {
    await navigator.clipboard.writeText(body.textContent);
    e.target.textContent = "Copied";
    setTimeout(() => { e.target.textContent = "Copy"; }, 1400);
  } catch {}
});

document.getElementById("installForm").addEventListener("submit", async (e) => {
  e.preventDefault();
  if (!invoke) return;

  const appleId = document.getElementById("appleIdInput").value.trim();
  const password = document.getElementById("passwordInput").value;
  const installBtn = document.getElementById("installBtn");
  const progressWrap = document.getElementById("progressWrap");
  const progressLabel = document.getElementById("progressLabel");
  const progressPct = document.getElementById("progressPct");
  const progressFill = document.getElementById("progressFill");
  const errorBox = document.getElementById("errorBox");
  const successBox = document.getElementById("successBox");
  const logsDrawer = document.getElementById("logsDrawer");
  const logsBody = document.getElementById("logsBody");

  errorBox.hidden = true;
  successBox.hidden = true;
  progressWrap.hidden = false;
  if (logsBody) logsBody.textContent = "";
  if (logsDrawer) logsDrawer.open = true;
  progressLabel.textContent = "Starting...";
  progressPct.textContent = "0%";
  progressFill.style.width = "0%";
  installBtn.disabled = true;

  try {
    await invoke("start_install", {
      appleId,
      password,
      profile: selectedProfile
    });
    progressLabel.textContent = "Complete";
    progressPct.textContent = "100%";
    progressFill.style.width = "100%";
    successBox.hidden = false;
    refreshInstallState();
  } catch (err) {
    const msg = String(err);
    appendLog(`ERROR: ${msg}`);
    if (msg.startsWith("APP_ID_LIMIT:")) {
      errorBox.innerHTML = `<strong>3-App-ID Limit Reached:</strong> ${msg.replace("APP_ID_LIMIT:", "")} <br>Switch to <strong>Share Only (2 IDs)</strong> or <strong>Core (1 ID)</strong> above and retry.`;
    } else {
      errorBox.textContent = msg;
    }
    errorBox.hidden = false;
  } finally {
    installBtn.disabled = false;
  }
});

if (listen) {
  listen("sideload-progress", (event) => {
    const pct = Math.max(0, Math.min(100, Math.round(event.payload.percent || 0)));
    document.getElementById("progressLabel").textContent = event.payload.stage || "Installing...";
    document.getElementById("progressPct").textContent = `${pct}%`;
    document.getElementById("progressFill").style.width = `${pct}%`;
  });

  listen("sideload-log", (event) => {
    appendLog(String(event.payload || ""));
  });

  listen("two-factor-required", () => {
    appendLog("Waiting for 6-digit 2FA code...");
    const modal = document.getElementById("twoFaModal");
    const input = document.getElementById("twoFaInput");
    input.value = "";
    modal.hidden = false;
    input.focus();
  });
}

document.getElementById("twoFaSubmit").addEventListener("click", async () => {
  const code = document.getElementById("twoFaInput").value.trim();
  document.getElementById("twoFaModal").hidden = true;
  if (invoke) await invoke("submit_two_factor_code", { code });
});

document.getElementById("twoFaCancel").addEventListener("click", async () => {
  document.getElementById("twoFaModal").hidden = true;
  if (invoke) await invoke("submit_two_factor_code", { code: "" });
});

async function refreshInstallState() {
  const badge = document.getElementById("expiryBadge");
  const fill = document.getElementById("expiryFill");
  const note = document.getElementById("expiryNote");
  const btn = document.getElementById("installBtn");
  const appleInput = document.getElementById("appleIdInput");
  if (!invoke) return;

  try {
    const state = await invoke("get_install_state");
    if (!state) {
      badge.textContent = "Not installed";
      fill.style.width = "0%";
      return;
    }

    if (state.appleId && !appleInput.value) {
      appleInput.value = state.appleId;
    }
    if (state.profile && PROFILE_IDS[state.profile]) {
      selectProfile(state.profile);
    }

    const nowSec = Math.floor(Date.now() / 1000);
    const totalSec = 7 * 86400;
    const remSec = state.expiresAt - nowSec;
    const pct = Math.max(0, Math.min(100, (remSec / totalSec) * 100));

    fill.classList.remove("is-warning", "is-expired");
    if (remSec <= 0) {
      badge.textContent = "Expired · Refresh needed";
      fill.style.width = "100%";
      fill.classList.add("is-expired");
    } else {
      const days = Math.floor(remSec / 86400);
      const hours = Math.floor((remSec % 86400) / 3600);
      badge.textContent = `${days}d ${hours}h left`;
      fill.style.width = `${pct.toFixed(1)}%`;
      if (days < 2) fill.classList.add("is-warning");
    }

    const expDate = new Date(state.expiresAt * 1000).toLocaleDateString();
    note.textContent = `Signed with ${state.appleId} (${state.profile}) · Expires ${expDate}. Keep FlowDownload on your iPhone and refresh below to keep all app data.`;
    btn.textContent = "Refresh 7-day signature";
  } catch {}
}

refreshRelease();
refreshInstallState();
refreshDevices();
setInterval(refreshDevices, 3000);
