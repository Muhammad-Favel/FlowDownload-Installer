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

  errorBox.hidden = true;
  successBox.hidden = true;
  progressWrap.hidden = false;
  progressLabel.textContent = "Downloading & signing...";
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
  } catch (err) {
    const msg = String(err);
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

  listen("two-factor-required", () => {
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

refreshRelease();
refreshDevices();
setInterval(refreshDevices, 3000);
