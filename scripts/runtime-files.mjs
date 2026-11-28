// 发行裁剪规则，保留安装树用于开发；功能替换须与插件组合和版本补丁配套。
export function excludedRuntimeFile(
  path,
  platform = process.platform,
  arch = process.arch,
) {
  const normalized = path.replaceAll('\\', '/');
  const name = normalized.split('/').at(-1);
  // 即使许可证放在异平台目录中也保留。
  if (/^(licen[cs]e|copying|copyright|notice)([.-]|$)/i.test(name))
    return undefined;
  if (
    /(?:^|\/)node_modules\/@deepseek-ai\/libreoffice-kit(?:-[^/]+)?\//.test(
      normalized,
    )
  )
    return 'libreoffice';
  if (
    /(?:^|\/)node_modules\/@deepseek-ai\/dsh-experimental-inspector\//.test(
      normalized,
    )
  )
    return 'experimental-inspector';
  if (/(?:^|\/)node_modules\/@vscode\/ripgrep(?:-[^/]+)?\//.test(normalized))
    return 'bundled-ripgrep';
  if (/\.(?:js|mjs|cjs|ts|mts|cts|css)\.map$/i.test(name)) return 'source-map';
  if (/\.pdb$/i.test(name)) return 'debug-symbols';
  if (/(?:^|\/)node_modules\/node-pty\/third_party\//.test(normalized))
    return 'node-pty-third-party';
  if (
    /(?:^|\/)node_modules\/@mixmark-io\/domino\/(?:test|\.yarn)\//.test(
      normalized,
    )
  )
    return 'package-dev-files';
  if (
    /(?:^|\/)node_modules\/@deepseek-ai\/[^/]+\/src\/.*\.(?:webp|png|jpe?g|gif|svg|woff2?|ttf|otf|ico)$/i.test(
      normalized,
    )
  )
    return 'duplicate-source-asset';
  const prebuild = normalized.match(
    /\/prebuilds\/(win32|linux|darwin)-(x64|arm64|ia32|arm)(?=[/+.-])/,
  );
  if (prebuild && (prebuild[1] !== platform || prebuild[2] !== arch))
    return 'foreign-prebuild';
  return undefined;
}
