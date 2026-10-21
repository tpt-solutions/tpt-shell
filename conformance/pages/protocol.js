// Reports the document type and the status of a request for a file that does not exist.
fetch("missing.txt").then(function (response) {
  document.title = location.origin + "|" + document.contentType + "|" + response.status;
}, function () {
  document.title = location.origin + "|" + document.contentType + "|error";
});
