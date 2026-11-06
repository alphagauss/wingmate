// 当前桌面前端只有静态启动页，只复制页面，不把包配置和说明文档嵌入程序。
import { mkdir, copyFile } from 'node:fs/promises';

await mkdir(new URL('./dist/', import.meta.url), { recursive: true });
await copyFile(new URL('./index.html', import.meta.url), new URL('./dist/index.html', import.meta.url));
