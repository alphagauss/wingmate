// 启动页与网关共用，连接面板由桌面壳使用。
import { mkdir, copyFile } from 'node:fs/promises';

await mkdir(new URL('./dist/_wingmate/', import.meta.url), { recursive: true });
await copyFile(new URL('../launcher/index.html', import.meta.url), new URL('./dist/index.html', import.meta.url));
await copyFile(new URL('../launcher/home.js', import.meta.url), new URL('./dist/_wingmate/home.js', import.meta.url));
await copyFile(new URL('./connections.html', import.meta.url), new URL('./dist/connections.html', import.meta.url));
await copyFile(new URL('./connections.js', import.meta.url), new URL('./dist/connections.js', import.meta.url));
await copyFile(new URL('../../src-tauri/icons/logo.svg', import.meta.url), new URL('./dist/_wingmate/logo.svg', import.meta.url));
