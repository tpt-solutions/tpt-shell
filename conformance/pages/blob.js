// Asks the host for a large string, eight times in a row. Each reply starts the next request, and
// the last reply reports the length, the last character and the count.
var COUNT = 8;
var got = 0;
window.chrome.webview.addEventListener("message", function (e) {
  got++;
  if (got < COUNT) {
    window.chrome.webview.postMessage("blob");
  } else {
    document.title = "blob:" + e.data.length + ":" + e.data.charAt(e.data.length - 1) + ":" + got;
  }
});
window.chrome.webview.postMessage("blob");
