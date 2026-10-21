// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

const form = document.getElementById("add");
const input = document.getElementById("text");
const list = document.getElementById("notes");
const status = document.getElementById("status");

function show(notes) {
  list.replaceChildren(
    ...notes.map((text) => {
      const li = document.createElement("li");
      li.textContent = text;
      return li;
    })
  );
}

if (!window.tpt) {
  status.textContent = "Open this page in the app window. A browser has no window.tpt.";
} else {
  window.tpt
    .invoke("notes.list")
    .then((reply) => show(reply.notes))
    .catch((error) => (status.textContent = `${error.code}: ${error.message}`));
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  const text = input.value.trim();
  if (text === "" || !window.tpt) return;
  window.tpt
    .invoke("notes.add", { text })
    .then(() => window.tpt.invoke("notes.list"))
    .then((reply) => {
      show(reply.notes);
      input.value = "";
      status.textContent = "";
    })
    .catch((error) => (status.textContent = `${error.code}: ${error.message}`));
});
