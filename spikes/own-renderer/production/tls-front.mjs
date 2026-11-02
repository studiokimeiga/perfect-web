// Track `deploy` (ADR-XXXX, a production host): **the TLS front of the local
// production smoke test**, Caddy's stand-in on a machine that has no Caddy.
// It terminates TLS with a certificate made for the run, and passes each
// request to the host as Caddy's `reverse_proxy` does: its method, target and
// fields as the browser sent them, `Host` among them, and each answer back as
// it streams, unbuffered.
//
//   node tls-front.mjs <listen port> <host port> <certificate> <key>
//
// It adds `X-Forwarded-For` and `X-Forwarded-Proto` as Caddy does (the host
// reads neither), and answers 502 where the host does not answer, as Caddy
// does while it restarts.
import { readFileSync } from "node:fs";
import http from "node:http";
import https from "node:https";

const [listen, upstream, cert, key] = process.argv.slice(2);
if (!listen || !upstream || !cert || !key) {
  console.error("usage: node tls-front.mjs <listen port> <host port> <certificate> <key>");
  process.exit(2);
}

// Fields that are a connection's, not a request's (RFC 9110 §7.6.1): never
// passed on.
const HOP = new Set([
  "connection",
  "keep-alive",
  "proxy-connection",
  "transfer-encoding",
  "te",
  "trailer",
  "upgrade",
]);

const server = https.createServer(
  { cert: readFileSync(cert), key: readFileSync(key) },
  (request, answer) => {
    const headers = {};
    for (const [name, value] of Object.entries(request.headers)) {
      if (!HOP.has(name)) headers[name] = value;
    }
    headers["x-forwarded-for"] = request.socket.remoteAddress ?? "";
    headers["x-forwarded-proto"] = "https";
    const passed = http.request(
      { host: "127.0.0.1", port: Number(upstream), method: request.method, path: request.url, headers },
      (from) => {
        const fields = {};
        for (const [name, value] of Object.entries(from.headers)) {
          if (!HOP.has(name)) fields[name] = value;
        }
        answer.writeHead(from.statusCode ?? 502, from.statusMessage, fields);
        from.pipe(answer);
      },
    );
    passed.on("error", () => {
      if (!answer.headersSent) {
        answer.writeHead(502, { "content-type": "text/plain; charset=utf-8" });
      }
      answer.end("the host does not answer");
    });
    request.pipe(passed);
  },
);
server.listen(Number(listen), "127.0.0.1", () => {
  console.log(`tls front on ${listen}, passing to ${upstream}`);
});
for (const signal of ["SIGTERM", "SIGINT"]) {
  process.on(signal, () => {
    server.close(() => process.exit(0));
    server.closeAllConnections();
    setTimeout(() => process.exit(0), 2000).unref();
  });
}
