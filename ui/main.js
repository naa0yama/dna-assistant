// Tauri IPC via window.__TAURI__ (withGlobalTauri: true)
const { invoke } = window.__TAURI__.core;

document.getElementById("greet-btn").addEventListener("click", async () => {
  const name = document.getElementById("greet-input").value || "World";
  const msg = await invoke("greet", { name });
  document.getElementById("greet-msg").textContent = msg;
});
