// Asks the host for a large string and reports its length and last character.
window.chrome.webview.addEventListener("message", function (e) {
  document.title = "blob:" + e.data.length + ":" + e.data.charAt(e.data.length - 1);
});
window.chrome.webview.postMessage("blob");
