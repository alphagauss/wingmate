// 只嵌入启动页和它使用的资源。
import { mkdir, copyFile } from 'node:fs/promises';

await mkdir(new URL('./dist/', import.meta.url), { recursive: true });
await copyFile(new URL('./index.html', import.meta.url), new URL('./dist/index.html', import.meta.url));
await copyFile(new URL('../../src-tauri/icons/logo.svg', import.meta.url), new URL('./dist/logo.svg', import.meta.url));
