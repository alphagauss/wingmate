// 仅过滤分发包中的调试信息和异平台预编译文件，保留安装树用于开发。
export function excludedRuntimeFile(path, platform = process.platform, arch = process.arch) {
  const normalized = path.replaceAll('\\', '/');
  const name = normalized.split('/').at(-1);
  // 即使许可证放在异平台目录中也保留。
  if (/^(licen[cs]e|copying|copyright|notice)([.-]|$)/i.test(name)) return undefined;
  if (/\.(?:js|mjs|cjs|ts|css)\.map$/i.test(name)) return 'source-map';
  if (/\.pdb$/i.test(name)) return 'debug-symbols';
  const prebuild = normalized.match(/\/prebuilds\/(win32|linux|darwin)-(x64|arm64|ia32|arm)(?=[/+.-])/);
  if (prebuild && (prebuild[1] !== platform || prebuild[2] !== arch)) return 'foreign-prebuild';
  return undefined;
}
