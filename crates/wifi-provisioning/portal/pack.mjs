import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

const [templatePath, cssPath, scriptPath, htmlOutput, rustOutput] = process.argv.slice(2);
if (!rustOutput) {
  throw new Error("usage: pack.mjs TEMPLATE CSS SCRIPT HTML_OUTPUT RUST_OUTPUT");
}

const [template, css, script] = await Promise.all([
  readFile(templatePath, "utf8"),
  readFile(cssPath, "utf8"),
  readFile(scriptPath, "utf8"),
]);

for (const marker of ["<!-- PORTAL_STYLES -->", "<!-- PORTAL_SCRIPT -->"]) {
  if (template.split(marker).length !== 2) throw new Error(`expected exactly one ${marker}`);
}
if (script.toLowerCase().includes("</script")) {
  throw new Error("the bundled script contains an HTML-closing script tag");
}

const compactTemplate = template
  .replace(/\s+/g, " ")
  .replace(/> </g, "><")
  .trim();
const html = compactTemplate
  .replace("<!-- PORTAL_STYLES -->", `<style>${css.trim()}</style>`)
  .replace("<!-- PORTAL_SCRIPT -->", `<script>${script.trim()}</script>`);

let hashes = 1;
while (html.includes(`"${"#".repeat(hashes)}`)) hashes += 1;
const fence = "#".repeat(hashes);
const rust = `#![no_std]\n\n/// Minified, self-contained captive-portal document.\npub const INDEX_HTML: &str = r${fence}"${html}"${fence};\n`;

await Promise.all([mkdir(dirname(htmlOutput), { recursive: true }), mkdir(dirname(rustOutput), { recursive: true })]);
await Promise.all([writeFile(htmlOutput, `${html}\n`), writeFile(rustOutput, rust)]);
