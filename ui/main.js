// Tauri IPC via window.__TAURI__ (withGlobalTauri: true)
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const MAX_LOG_ENTRIES = 200;

// --- DOM elements ---
const sidebar = document.getElementById("sidebar");
const sidebarToggle = document.getElementById("sidebar-toggle");
const navItems = document.querySelectorAll(".nav-item");
const pages = document.querySelectorAll(".page");

// Main page
const statusBadge = document.getElementById("status-badge");
const framesCount = document.getElementById("frames-count");
const eventsCount = document.getElementById("events-count");
const startBtn = document.getElementById("start-btn");
const stopBtn = document.getElementById("stop-btn");
const clearLogBtn = document.getElementById("clear-log-btn");
const eventLog = document.getElementById("event-log");

// Main page detector badges
const detSkill = document.getElementById("det-skill");
const detRound = document.getElementById("det-round");
const detDialog = document.getElementById("det-dialog");

// Log page
const clearLogFullBtn = document.getElementById("clear-log-full-btn");
const eventLogFull = document.getElementById("event-log-full");

// Detection page
const capWindow = document.getElementById("cap-window");
const capSize = document.getElementById("cap-size");
const capBackend = document.getElementById("cap-backend");
const detSkillFull = document.getElementById("det-skill-full");
const detRoundFull = document.getElementById("det-round-full");
const detDialogFull = document.getElementById("det-dialog-full");
const detSkillTime = document.getElementById("det-skill-time");
const detRoundTime = document.getElementById("det-round-time");
const detDialogTime = document.getElementById("det-dialog-time");

// --- Sidebar toggle ---
sidebarToggle.addEventListener("click", () => {
  sidebar.classList.toggle("collapsed");
});

// --- Page navigation ---
navItems.forEach((item) => {
  item.addEventListener("click", () => {
    const target = item.dataset.page;
    navItems.forEach((n) => n.classList.remove("active"));
    item.classList.add("active");
    pages.forEach((p) => {
      p.classList.toggle("hidden", p.id !== "page-" + target);
    });
  });
});

// --- Status badge styles ---
const STATE_STYLES = {
  idle: { text: "Idle", cls: "badge-ghost" },
  searching_window: { text: "Searching...", cls: "badge-warning" },
  capturing: { text: "Capturing", cls: "badge-success" },
};

function updateStatusUI(status) {
  const style = STATE_STYLES[status.state] || STATE_STYLES.idle;
  statusBadge.textContent = style.text;
  statusBadge.className = "badge " + style.cls;
  framesCount.textContent = status.frames_captured;
  eventsCount.textContent = status.events_detected;

  // Frame timing (Detection page only)
  const capFrameTiming = document.getElementById("cap-frame-timing");
  if (status.state === "capturing" && status.fps > 0) {
    capFrameTiming.textContent = status.frame_time_ms.toFixed(1) + "ms | " + status.fps.toFixed(1) + "fps";
  } else {
    capFrameTiming.textContent = "";
  }

  const isActive = status.state !== "idle";
  startBtn.disabled = isActive;
  stopBtn.disabled = !isActive;
}

// --- Detector state tracking ---
const detectorState = {
  skill: { state: "unknown", label: "--", time: null },
  round: { state: "unknown", label: "--", time: null },
  dialog: { state: "unknown", label: "--", time: null },
};

function updateDetectorBadge(el, state, label) {
  el.dataset.state = state;
  el.textContent = label;
}

function syncDetectorUI() {
  const s = detectorState;
  // Main page badges
  updateDetectorBadge(detSkill, s.skill.state, s.skill.label);
  updateDetectorBadge(detRound, s.round.state, s.round.label);
  updateDetectorBadge(detDialog, s.dialog.state, s.dialog.label);
  // Detection page badges
  updateDetectorBadge(detSkillFull, s.skill.state, s.skill.label);
  updateDetectorBadge(detRoundFull, s.round.state, s.round.label);
  updateDetectorBadge(detDialogFull, s.dialog.state, s.dialog.label);
  // Detection page times
  detSkillTime.textContent = s.skill.time || "--";
  detRoundTime.textContent = s.round.time || "--";
  detDialogTime.textContent = s.dialog.time || "--";
}

function updateDetectorFromEvent(kind) {
  const now = new Date().toLocaleTimeString("ja-JP", { hour12: false });
  switch (kind) {
    case "SkillReady":
      detectorState.skill = { state: "ok", label: "Ready", time: now };
      break;
    case "SkillGreyed":
      detectorState.skill = { state: "warn", label: "Greyed", time: now };
      break;
    case "RoundVisible":
      detectorState.round = { state: "ok", label: "Visible", time: now };
      break;
    case "RoundGone":
      detectorState.round = { state: "unknown", label: "Gone", time: now };
      break;
    case "DialogVisible":
      detectorState.dialog = { state: "alert", label: "Visible", time: now };
      break;
    case "DialogGone":
      detectorState.dialog = { state: "unknown", label: "None", time: now };
      break;
  }
  syncDetectorUI();
}

// --- Log entries ---
function createLogEntry(kind, detail) {
  const now = new Date().toLocaleTimeString("ja-JP", { hour12: false });
  const entry = document.createElement("div");
  entry.className = "log-entry";
  entry.innerHTML =
    '<span class="log-time">' + now + "</span>" +
    '<span class="log-kind" data-kind="' + kind + '">' + kind + "</span>" +
    '<span class="log-msg">' + detail + "</span>";
  return entry;
}

function addLogEntry(kind, detail) {
  // Remove placeholder
  for (const log of [eventLog, eventLogFull]) {
    const ph = log.querySelector("p");
    if (ph) ph.remove();
  }

  const entry1 = createLogEntry(kind, detail);
  const entry2 = createLogEntry(kind, detail);
  eventLog.prepend(entry1);
  eventLogFull.prepend(entry2);

  // Trim
  while (eventLog.children.length > MAX_LOG_ENTRIES) eventLog.lastChild.remove();
  while (eventLogFull.children.length > MAX_LOG_ENTRIES) eventLogFull.lastChild.remove();
}

function clearLog() {
  const placeholder = '<p class="text-sm text-base-content/30 p-2">No events yet</p>';
  eventLog.innerHTML = placeholder;
  eventLogFull.innerHTML = placeholder;
}

// --- Button handlers ---
startBtn.addEventListener("click", async () => {
  try { await invoke("start_monitoring"); } catch (e) { console.error("start failed:", e); }
});
stopBtn.addEventListener("click", async () => {
  try { await invoke("stop_monitoring"); } catch (e) { console.error("stop failed:", e); }
});
clearLogBtn.addEventListener("click", clearLog);
clearLogFullBtn.addEventListener("click", clearLog);

// --- Tauri event listeners ---
listen("monitor-status", (event) => {
  updateStatusUI(event.payload);
});

listen("detection-event", (event) => {
  const { kind, detail } = event.payload;
  addLogEntry(kind, detail);
  updateDetectorFromEvent(kind);
});

// --- Capture preview (Detection page) ---
const capturePreview = document.getElementById("capture-preview");
const capturePlaceholder = document.getElementById("capture-placeholder");
const capImgSize = document.getElementById("cap-img-size");
let previewInterval = null;

async function refreshCapturePreview() {
  try {
    const data = await invoke("get_capture_preview");
    // Update capture info
    if (data.info.window_name) {
      capWindow.textContent = data.info.window_name;
      capSize.textContent = data.info.width + " x " + data.info.height;
      capBackend.textContent = data.info.backend;
    }
    // Update image
    if (data.image_base64) {
      capturePreview.src = "data:image/png;base64," + data.image_base64;
      capturePreview.classList.remove("hidden");
      capturePlaceholder.classList.add("hidden");
      capImgSize.textContent = data.info.width + " x " + data.info.height + " px";
      capImgSize.classList.remove("hidden");
    }
  } catch (e) {
    console.error("get_capture_preview failed:", e);
  }
}

// Poll preview when Detection page is visible
async function startPreviewPolling() {
  if (previewInterval) return;
  // Use preview_interval from settings, fallback 3000ms
  let interval = 3000;
  try {
    const cfg = await invoke("get_settings");
    if (cfg.preview_interval > 0) interval = cfg.preview_interval;
  } catch (_) { /* use default */ }
  refreshCapturePreview();
  previewInterval = setInterval(refreshCapturePreview, interval);
}

function stopPreviewPolling() {
  if (previewInterval) {
    clearInterval(previewInterval);
    previewInterval = null;
  }
}

// Hook into page navigation to start/stop polling + settings load
navItems.forEach((item) => {
  item.addEventListener("click", () => {
    if (item.dataset.page === "detection") {
      startPreviewPolling();
    } else {
      stopPreviewPolling();
    }
    if (item.dataset.page === "settings") {
      loadSettings();
    }
  });
});

// --- Settings ---
const settingsInputs = document.querySelectorAll("#page-settings input[data-key]");
const settingsSaveBtn = document.getElementById("settings-save-btn");
const settingsResetBtn = document.getElementById("settings-reset-btn");

function populateSettings(config) {
  for (const input of settingsInputs) {
    const key = input.dataset.key;
    if (key in config) {
      input.value = config[key];
    }
  }
}

// Fields serialized as ms (integer) vs sec (float)
const MS_KEYS = new Set([
  "capture_interval", "window_search_interval", "preview_interval", "skill_debounce",
]);

function collectSettings() {
  const config = {};
  for (const input of settingsInputs) {
    const key = input.dataset.key;
    const val = parseFloat(input.value);
    if (key === "max_capture_retries" || MS_KEYS.has(key)) {
      config[key] = Math.round(val);
    } else {
      config[key] = val;
    }
  }
  return config;
}

async function loadSettings() {
  try {
    const config = await invoke("get_settings");
    populateSettings(config);
  } catch (e) {
    console.error("get_settings failed:", e);
  }
}

settingsSaveBtn.addEventListener("click", async () => {
  try {
    const config = collectSettings();
    await invoke("save_settings", { config });
    settingsSaveBtn.textContent = "Saved!";
    setTimeout(() => { settingsSaveBtn.textContent = "Save"; }, 1500);
  } catch (e) {
    console.error("save_settings failed:", e);
  }
});

settingsResetBtn.addEventListener("click", async () => {
  try {
    const defaults = await invoke("get_default_settings");
    populateSettings(defaults);
    await invoke("save_settings", { config: defaults });
    settingsResetBtn.textContent = "Reset!";
    setTimeout(() => { settingsResetBtn.textContent = "Reset to defaults"; }, 1500);
  } catch (e) {
    console.error("reset failed:", e);
  }
});

// --- Initial status ---
invoke("get_status").then(updateStatusUI).catch(console.error);
