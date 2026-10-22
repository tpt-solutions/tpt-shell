// Fetches the same blob eight times in a row, from the app's own blob URL, the way a page reads a
// blob it was handed. Each read is split in two: the response (headers) and the body (the bytes).
// The title reports the length, the last byte, the count, and the medians in milliseconds of the
// whole read and of the response alone.
var COUNT = 8;
var totals = [];
var headers = [];
var length = 0;
var last = "";

function median(values) {
  var sorted = values.slice().sort(function (a, b) {
    return a - b;
  });
  return sorted[sorted.length >> 1];
}

function next() {
  var started = performance.now();
  fetch("__tpt/blob/big").then(function (response) {
    var responded = performance.now();
    response.arrayBuffer().then(function (bytes) {
      var done = performance.now();
      headers.push(responded - started);
      totals.push(done - started);
      var view = new Uint8Array(bytes);
      length = view.length;
      last = String.fromCharCode(view[view.length - 1]);
      if (totals.length < COUNT) {
        next();
      } else {
        document.title =
          "fetch:" + length + ":" + last + ":" + totals.length + ":" +
          median(totals).toFixed(3) + ":" + median(headers).toFixed(3);
      }
    });
  });
}

next();
