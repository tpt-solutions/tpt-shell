// Round trips: each reply to a ping starts the next ping, until the count is reached.
var ROUNDS = 200;
var seen = 0;
window.chrome.webview.addEventListener("message", function (e) {
  seen++;
  if (seen < ROUNDS) {
    window.chrome.webview.postMessage("ping");
  } else {
    document.title = "done:" + seen + ":" + e.data;
  }
});
window.chrome.webview.postMessage("ping");
