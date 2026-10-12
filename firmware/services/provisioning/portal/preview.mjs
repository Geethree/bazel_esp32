import { readFile } from "node:fs/promises";
import { createServer } from "node:http";

const [htmlPath] = process.argv.slice(2);
if (!htmlPath) throw new Error("missing generated HTML runfile path");
const html = await readFile(htmlPath);
const port = Number.parseInt(process.env.PORT || "4173", 10);
const networks = [
  { ssid: "Greenhouse", rssi: -38, secure: true },
  { ssid: "Studio Wi-Fi", rssi: -57, secure: true },
  { ssid: "Guest Network", rssi: -68, secure: false },
  { ssid: "Workshop", rssi: -79, secure: true },
];
let status = { state: "ready" };

function reply(response, code, type, body) {
  response.writeHead(code, { "Content-Type": type, "Cache-Control": "no-store" });
  response.end(body);
}

const server = createServer((request, response) => {
  const url = new URL(request.url, `http://${request.headers.host || "localhost"}`);
  if (request.method === "GET" && url.pathname === "/") {
    reply(response, 200, "text/html; charset=utf-8", html);
  } else if (request.method === "GET" && url.pathname === "/networks") {
    reply(response, 200, "application/json", JSON.stringify(networks));
  } else if (request.method === "GET" && url.pathname === "/status") {
    reply(response, 200, "application/json", JSON.stringify(status));
  } else if (request.method === "POST" && url.pathname === "/connect") {
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk) => { body += chunk; });
    request.on("end", () => {
      const form = new URLSearchParams(body);
      if (!form.get("ssid")) {
        reply(response, 400, "application/json", JSON.stringify({ message: "Network name is required." }));
        return;
      }
      status = { state: "connecting" };
      setTimeout(() => { status = { state: "connected" }; }, 1800);
      reply(response, 202, "application/json", JSON.stringify({ message: "Credentials accepted; connecting." }));
    });
  } else if (url.pathname === "/favicon.ico") {
    response.writeHead(204).end();
  } else {
    response.writeHead(302, { Location: "/" }).end();
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`Portal preview: http://127.0.0.1:${port}/`);
});
