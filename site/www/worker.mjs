// www.pleris.dev: every request answered with a permanent redirect to the
// same path and query on pleris.dev.
export default {
  fetch(request) {
    const url = new URL(request.url);
    url.protocol = "https:";
    url.hostname = "pleris.dev";
    url.port = "";
    return Response.redirect(url.href, 301);
  },
};
