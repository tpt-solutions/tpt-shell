// Tries to leave the app origin. The check then reads the origin and expects the app origin.
setTimeout(function () {
  location.href = "https://example.com/";
}, 100);
