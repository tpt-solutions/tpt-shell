// Fetches the same blob eight times in a row, from the app's own blob URL, the way a page reads a
// blob it was handed. Each timing runs from the request to the bytes being read. The title reports
// the length, the last byte, the count and the median time in milliseconds.
var COUNT = 8;
var times = [];
var length = 0;
var last = "";

function next() {
  var started = performance.now();
  fetch("__tpt/blob/big")
    .then(function (response) {
      return response.arrayBuffer();
    })
    .then(function (bytes) {
      times.push(performance.now() - started);
      var view = new Uint8Array(bytes);
      length = view.length;
      last = String.fromCharCode(view[view.length - 1]);
      if (times.length < COUNT) {
        next();
      } else {
        times.sort(function (a, b) {
          return a - b;
        });
        var median = times[times.length >> 1];
        document.title = "fetch:" + length + ":" + last + ":" + times.length + ":" + median.toFixed(3);
      }
    });
}

next();
