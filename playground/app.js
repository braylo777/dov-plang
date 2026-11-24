// .dov playground glue: plain WebAssembly, no wasm-bindgen, no npm.
"use strict";
let wasm = null;
const $ = (id) => document.getElementById(id);
const enc = new TextEncoder();
const dec = new TextDecoder();

function call(src, mode) {
  const e = wasm.exports;
  const bytes = enc.encode(src);
  const ptr = e.dov_alloc(bytes.length);
  new Uint8Array(e.memory.buffer, ptr, bytes.length).set(bytes);
  const status = e.dov_run(ptr, bytes.length, mode);
  const out = dec.decode(new Uint8Array(e.memory.buffer, e.dov_out_ptr(), e.dov_out_len()));
  return { ok: status === 0, out };
}

function compile() {
  if (!wasm) return;
  const t0 = performance.now();
  const r = call($("src").value, 0);
  const ms = (performance.now() - t0).toFixed(1);
  $("status").textContent = r.ok ? `ok · ${ms} ms` : `error · ${ms} ms`;
  $("status").className = r.ok ? "ok" : "bad";
  $("diag").textContent = r.ok ? "no errors" : r.out;
  $("diag").style.color = r.ok ? "var(--good)" : "var(--bad)";
  $("c").textContent = r.ok ? r.out : "";
}

function format() {
  if (!wasm) return;
  const r = call($("src").value, 1);
  if (r.ok) $("src").value = r.out;
  compile();
}

function load(i) {
  const p = PRESETS[i];
  $("src").value = p.source;
  $("out").textContent = p.output || "(does not compile; see the diagnostic)";
  document.querySelectorAll("#presets button").forEach((b, k) => b.classList.toggle("on", k === i));
  compile();
}

PRESETS.forEach((p, i) => {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = `${i + 1}. ${p.title}`;
  b.onclick = () => load(i);
  $("presets").appendChild(b);
});
$("go").onclick = compile;
$("fmt").onclick = format;
let timer = 0;
$("src").addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(compile, 250); $("out").textContent = "(edited; native output not available)"; });

WebAssembly.instantiateStreaming(fetch("dovc.wasm"), {})
  .catch(() => fetch("dovc.wasm").then((r) => r.arrayBuffer()).then((b) => WebAssembly.instantiate(b, {})))
  .then((m) => { wasm = m.instance; load(0); })
  .catch((err) => { $("diag").textContent = "could not load dovc.wasm: " + err; });
