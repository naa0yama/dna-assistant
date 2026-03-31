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
const detRoundtrip = document.getElementById("det-roundtrip");
const detRound = document.getElementById("det-round");
const detDialog = document.getElementById("det-dialog");

// Log page
const clearLogFullBtn = document.getElementById("clear-log-full-btn");
const eventLogFull = document.getElementById("event-log-full");

// Detection page
const capWindow = document.getElementById("cap-window");
const capSize = document.getElementById("cap-size");
const capBackend = document.getElementById("cap-backend");
const detRoundFull = document.getElementById("det-round-full");
const detDialogFull = document.getElementById("det-dialog-full");
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

const ocrStatus = document.getElementById("ocr-status");

function updateStatusUI(status) {
  const style = STATE_STYLES[status.state] || STATE_STYLES.idle;
  statusBadge.textContent = style.text;
  statusBadge.className = "badge " + style.cls;
  framesCount.textContent = status.frames_captured;
  eventsCount.textContent = status.events_detected;

  // OCR status
  if (status.state === "idle") {
    ocrStatus.textContent = "--";
    ocrStatus.className = "text-base-content/40";
  } else if (status.ocr_available) {
    ocrStatus.textContent = "Available";
    ocrStatus.className = "text-success";
  } else {
    ocrStatus.textContent = "Unavailable";
    ocrStatus.className = "text-warning";
  }

  // Resolution warning
  const resWarning = document.getElementById("resolution-warning");
  const resWarningText = document.getElementById("resolution-warning-text");
  if (status.resolution_warning) {
    resWarningText.textContent = status.resolution_warning;
    resWarning.classList.remove("hidden");
  } else {
    resWarning.classList.add("hidden");
  }

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
  round: { state: "unknown", label: "--", time: null },
  dialog: { state: "unknown", label: "--", time: null },
};

// --- Detector enabled state ---
const detectorEnabled = { round: true, dialog: true };

// --- RoundTrip state ---
let roundtripStartTime = null;
let roundtripTimerId = null;
let roundtripConfig = { green: 60, yellow: 120, red: 180 };

function updateDetectorEnabledState(config) {
  detectorEnabled.round = config.round_enabled !== false;
  detectorEnabled.dialog = config.dialog_enabled !== false;
  // Cache RoundTrip thresholds
  roundtripConfig.green = config.roundtrip_green || 60;
  roundtripConfig.yellow = config.roundtrip_yellow || 120;
  roundtripConfig.red = config.roundtrip_red || 180;
  syncDetectorUI();
}

function updateDetectorBadge(el, state, label) {
  el.dataset.state = state;
  el.textContent = label;
}

function formatElapsed(secs) {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return m > 0 ? m + "m " + String(s).padStart(2, "0") + "s" : s + "s";
}

function getRoundtripState(secs) {
  if (secs >= roundtripConfig.yellow) return "alert";
  if (secs >= roundtripConfig.green) return "warn";
  return "ok";
}

function updateRoundtripBadge() {
  if (roundtripStartTime == null) return;
  const secs = (Date.now() - roundtripStartTime) / 1000;
  const state = getRoundtripState(secs);
  updateDetectorBadge(detRoundtrip, state, formatElapsed(secs));
}

function startRoundtripTimer() {
  roundtripStartTime = Date.now();
  if (roundtripTimerId) clearInterval(roundtripTimerId);
  roundtripTimerId = setInterval(updateRoundtripBadge, 1000);
  updateRoundtripBadge();
}

function stopRoundtripTimer(elapsedSecs) {
  if (roundtripTimerId) {
    clearInterval(roundtripTimerId);
    roundtripTimerId = null;
  }
  if (elapsedSecs != null) {
    const state = getRoundtripState(elapsedSecs);
    updateDetectorBadge(detRoundtrip, state, formatElapsed(elapsedSecs));
  }
  roundtripStartTime = null;
}

function syncDetectorUI() {
  const s = detectorState;
  // Main page badges
  if (detectorEnabled.round) {
    updateDetectorBadge(detRound, s.round.state, s.round.label);
  } else {
    updateDetectorBadge(detRound, "disabled", "Disabled");
  }
  if (detectorEnabled.dialog) {
    updateDetectorBadge(detDialog, s.dialog.state, s.dialog.label);
  } else {
    updateDetectorBadge(detDialog, "disabled", "Disabled");
  }
  // Detection page badges
  if (detectorEnabled.round) {
    updateDetectorBadge(detRoundFull, s.round.state, s.round.label);
  } else {
    updateDetectorBadge(detRoundFull, "disabled", "Disabled");
  }
  if (detectorEnabled.dialog) {
    updateDetectorBadge(detDialogFull, s.dialog.state, s.dialog.label);
  } else {
    updateDetectorBadge(detDialogFull, "disabled", "Disabled");
  }
  // Detection page times
  detRoundTime.textContent = s.round.time || "--";
  detDialogTime.textContent = s.dialog.time || "--";
}

function updateDetectorFromEvent(kind, elapsedSecs) {
  const now = new Date().toLocaleTimeString("ja-JP", { hour12: false });
  switch (kind) {
    case "RoundVisible":
      detectorState.round = { state: "ok", label: "Visible", time: now };
      startRoundtripTimer();
      break;
    case "RoundGone":
      detectorState.round = { state: "unknown", label: "Gone", time: now };
      stopRoundtripTimer(elapsedSecs);
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
function createLogEntry(kind, detail, roundNumber, elapsed) {
  const now = new Date().toLocaleTimeString("ja-JP", { hour12: false });
  const numStr = roundNumber != null ? String(roundNumber) : "";
  const elapsedStr = elapsed || "";
  const entry = document.createElement("div");
  entry.className = "log-entry";
  entry.innerHTML =
    '<span class="log-time">' + now + "</span>" +
    '<span class="log-elapsed">' + elapsedStr + "</span>" +
    '<span class="log-kind" data-kind="' + kind + '">' + kind + "</span>" +
    '<span class="log-num">' + numStr + "</span>" +
    '<span class="log-msg">' + detail + "</span>";
  return entry;
}

function addLogEntry(kind, detail, roundNumber, elapsed) {
  // Remove placeholder
  for (const log of [eventLog, eventLogFull]) {
    const ph = log.querySelector("p");
    if (ph) ph.remove();
  }

  const entry1 = createLogEntry(kind, detail, roundNumber, elapsed);
  const entry2 = createLogEntry(kind, detail, roundNumber, elapsed);
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
  const { kind, detail, round_number, elapsed, elapsed_secs } = event.payload;
  addLogEntry(kind, detail, round_number, elapsed);
  updateDetectorFromEvent(kind, elapsed_secs);
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
      if (input.type === "checkbox") {
        input.checked = config[key];
      } else {
        input.value = config[key];
      }
    }
  }
}

// Fields serialized as ms (integer) vs sec (float)
const MS_KEYS = new Set([
  "capture_interval", "window_search_interval", "preview_interval",
]);
const INT_KEYS = new Set(["max_capture_retries"]);
const STRING_KEYS = new Set(["discord_webhook_url", "discord_mention_id"]);

// Sidebar Discord toggle (outside settings form)
const discordToggle = document.getElementById("discord-toggle");

function collectSettings() {
  const config = {};
  for (const input of settingsInputs) {
    const key = input.dataset.key;
    if (input.type === "checkbox") {
      config[key] = input.checked;
    } else if (STRING_KEYS.has(key)) {
      config[key] = input.value;
    } else {
      const val = parseFloat(input.value);
      if (INT_KEYS.has(key) || MS_KEYS.has(key)) {
        config[key] = Math.round(val);
      } else {
        config[key] = val;
      }
    }
  }
  // Sync discord_enabled from sidebar toggle
  config.discord_enabled = discordToggle.checked;
  return config;
}

async function loadSettings() {
  try {
    const config = await invoke("get_settings");
    populateSettings(config);
    updateDetectorEnabledState(config);
    // Sync sidebar Discord toggle
    discordToggle.checked = config.discord_enabled || false;
  } catch (e) {
    console.error("get_settings failed:", e);
  }
}

settingsSaveBtn.addEventListener("click", async () => {
  try {
    const config = collectSettings();
    await invoke("save_settings", { config });
    updateDetectorEnabledState(config);
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

// --- Test notification ---
const testNotificationBtn = document.getElementById("test-notification-btn");
testNotificationBtn.addEventListener("click", async () => {
  try {
    await invoke("test_notification");
    testNotificationBtn.textContent = "Sent!";
    setTimeout(() => { testNotificationBtn.textContent = "Test Notification"; }, 1500);
  } catch (e) {
    console.error("test_notification failed:", e);
  }
});

// --- Sidebar Discord toggle auto-save ---
discordToggle.addEventListener("change", async () => {
  try {
    const config = await invoke("get_settings");
    config.discord_enabled = discordToggle.checked;
    await invoke("save_settings", { config });
  } catch (e) {
    console.error("discord toggle save failed:", e);
  }
});

// --- Initial status ---
invoke("get_status").then(updateStatusUI).catch(console.error);
invoke("get_settings").then((config) => {
  updateDetectorEnabledState(config);
  discordToggle.checked = config.discord_enabled || false;
}).catch(console.error);
