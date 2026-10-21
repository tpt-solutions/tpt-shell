// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Shows the last entries of the audit log, or the reason the chain does not check.
const chain = document.getElementById("chain");
const rows = document.getElementById("rows");

function cell(text) {
  const td = document.createElement("td");
  td.textContent = text === undefined || text === null ? "" : String(text);
  return td;
}

function show(reply) {
  chain.textContent = `Chain ok: ${reply.records} records, ${reply.checkpoints} checkpoints, last hash ${reply.last_hash.slice(0, 16)}...`;
  rows.replaceChildren(
    ...reply.entries.map((entry) => {
      const tr = document.createElement("tr");
      tr.append(
        cell(entry.seq),
        cell(entry.command),
        cell(entry.outcome),
        cell(JSON.stringify(entry))
      );
      return tr;
    })
  );
}

if (!window.tpt) {
  chain.textContent = "Open this page in the app window.";
} else {
  window.tpt
    .invoke("audit.tail", { limit: 50 })
    .then(show)
    .catch((error) => {
      chain.textContent = `${error.code}: ${error.message}`;
    });
}
