// KaTeX's verdict on formulas (T2.7h.34): reads JSON lines with `tex` and
// `display` on stdin, writes each line back with `katex` set to `ok` or
// KaTeX's message. Needs the `katex` package (`npm install katex`).
const katex = require("katex");
const readline = require("readline");
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => {
  if (!line.trim()) return;
  const d = JSON.parse(line);
  try {
    katex.renderToString(d.tex, { displayMode: !!d.display, throwOnError: true, strict: "ignore", trust: true });
    d.katex = "ok";
  } catch (e) {
    d.katex = String(e.message || e).split("\n")[0];
  }
  process.stdout.write(JSON.stringify(d) + "\n");
});
