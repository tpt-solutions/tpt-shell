// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// A DOM front end with no build step. The shell's runtime sets window.tpt before this runs.
// Tasks are kept in memory only; a real app would keep them behind a command.

const status = document.getElementById("status");
const form = document.getElementById("task-form");
const input = document.getElementById("task-text");
const list = document.getElementById("tasks");

/** @type {{ id: number, text: string, done: boolean }[]} */
const tasks = [];
let nextId = 1;

function render() {
  list.replaceChildren(
    ...tasks.map((task) => {
      const item = document.createElement("li");
      const label = document.createElement("label");
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = task.done;
      box.addEventListener("change", () => {
        task.done = box.checked;
        render();
      });
      label.append(box, " ", task.text);
      item.append(label);
      return item;
    })
  );
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  const text = input.value.trim();
  if (text === "") return;
  tasks.push({ id: nextId++, text, done: false });
  input.value = "";
  render();
});

if (window.tpt) {
  window.tpt
    .invoke("app.info")
    .then((info) => {
      status.textContent = `${info.name} ${info.version}`;
    })
    .catch((error) => {
      status.textContent = `${error.code}: ${error.message}`;
    });
} else {
  status.textContent = "Open this page in the app window. A browser has no window.tpt.";
}
