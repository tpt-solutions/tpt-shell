// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Keeps focus inside the dialog and never lets Tab leave it. This is the keyboard trap.
// Served as a file: the shell's page policy blocks inline script.
const dialog = document.getElementById('seed-trap-dialog');
document.getElementById('seed-trap-open').addEventListener('click', () => dialog.showModal());
dialog.addEventListener('keydown', (event) => {
  if (event.key !== 'Tab') return;
  event.preventDefault();
  const first = document.getElementById('trap-a');
  const second = document.getElementById('trap-b');
  (document.activeElement === first ? second : first).focus();
});
