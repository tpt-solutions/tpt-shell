// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// <tpt-progress>: an optional progress bar for a job (spec §7.2). Not part of the runtime, so it
// does not count against the runtime's size budget. Load it after the runtime:
//
//   <script src="tpt-progress.js"></script>
//   <tpt-progress></tpt-progress>
//   <script>document.querySelector('tpt-progress').track(api.scan({ root }));</script>
//
// `track(handle)` takes the job handle that a job command resolves to (`{ id, cancel, on }`
// from tpt.job, or the generated client). The element shows `job.progress` as a percentage,
// and marks itself done on `job.result` or `job.failed`/`job.cancelled`. A `cancel` button
// inside the element, if present, calls `handle.cancel()`.
(function () {
  if (!window.customElements || !window.HTMLElement) {
    return;
  }

  class TptProgress extends window.HTMLElement {
    connectedCallback() {
      if (!this.getAttribute('role')) {
        this.setAttribute('role', 'progressbar');
      }
      this.setAttribute('aria-valuemin', '0');
      this.setAttribute('aria-valuemax', '100');
      this.setAttribute('aria-valuenow', '0');
    }

    /** Follows a job handle, and returns a function that stops following it. */
    track(handle) {
      const stops = [];
      const set = (value) => {
        this.setAttribute('aria-valuenow', String(value));
        this.setAttribute('data-value', String(value));
      };
      this.setAttribute('data-state', 'running');
      stops.push(
        handle.on('job.progress', (d) => {
          if (d.total > 0) {
            set(Math.min(100, Math.round((d.done / d.total) * 100)));
          }
        })
      );
      const finish = (state) => () => {
        this.setAttribute('data-state', state);
        if (state === 'result') {
          set(100);
        }
        stops.splice(0).forEach((stop) => stop());
      };
      stops.push(handle.on('job.result', finish('result')));
      stops.push(handle.on('job.failed', finish('failed')));
      stops.push(handle.on('job.cancelled', finish('cancelled')));
      return () => stops.splice(0).forEach((stop) => stop());
    }
  }

  if (!window.customElements.get('tpt-progress')) {
    window.customElements.define('tpt-progress', TptProgress);
  }
  window.TptProgress = TptProgress;
})();
