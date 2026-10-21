// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The start screen for a kiosk. The clock needs no command, so the page works without a backend.
const clock = document.getElementById("clock");
const start = document.getElementById("start");

function tick() {
  clock.textContent = new Date().toLocaleTimeString();
}
tick();
setInterval(tick, 1000);

start.addEventListener("click", () => {
  start.textContent = "Started";
  start.disabled = true;
});
