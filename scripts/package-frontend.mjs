import assert from 'node:assert/strict';
import { cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

// 保留 Dioxus 已解析资源引用的 JS/Wasm，不能重新绑定 Cargo 的原始 Wasm。
const source = process.argv[2];
const html = await readFile(join(source, 'index.html'), 'utf8');
const entries = [...html.matchAll(/href="(\/\.\/assets\/aio-plugin-file-client-[^"]+\.js)"/g)];
assert.equal(entries.length, 1, 'Dioxus 应生成唯一的入口脚本');
const entry = await readFile(join(source, entries[0][1].slice(3)), 'utf8');
// Dioxus 0.7 的启动地址以根路径开头；整包必须相对于当前挂载票据加载。
const wasm = /module_or_path:"\/\.\/assets\/([^"/]+\.wasm)"/g;
assert.equal([...entry.matchAll(wasm)].length, 1, 'Dioxus 应生成唯一的 Wasm 启动地址');
await mkdir('dist/frontend', { recursive: true });
await cp(join(source, 'assets'), 'dist/frontend/assets', { recursive: true });
await cp('client/index.html', 'dist/frontend/index.html');
await writeFile('dist/frontend/bootstrap.js', entry.replace(wasm, 'module_or_path:"./assets/$1"'));
