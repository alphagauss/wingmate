# 运行时调试

先用源码、静态检查和相关测试缩小范围。需要运行时证据时，明确复现条件、需要观察的状态和复验标准。按 [验证约定](../scripts/README.md) 隔离测试数据。

| 排查对象 | 首选证据 |
| --- | --- |
| 后台启动、身份、复用和退出 | CLI 的状态和退出码、`service.log`、对应 Node PID |
| 首页、DSH 工作台、后台服务设置和 Tauri 桥接 | 真实 Tauri WebView2 的 CDP。检查 DOM、Console、Network，按需在页面内操作并复查状态 |
| 系统对话框、窗口焦点和默认浏览器交接 | 先查相关测试、原生日志和进程证据。实际外观和焦点优先请用户人工复验 |

只有确实需要原生界面证据，且上述手段与人工反馈都无法提供时，才使用 Computer Use。先说明缺少什么证据，只执行必需的原生步骤。CDP 连不上时先查启动输出、监听端口、WebView2 进程和数据目录。

## Windows 开发窗口

在当前工作副本根目录启动 dev 模式，复用已有的编译产物和运行时安装树。开发后台、DSH 数据和 WebView 资料都在当前工作副本的 `work/dev` 下，与发行版隔离。可按任务需要启停开发后台、修改或清理开发模式产生的数据。清理前先退出正在使用这些数据的开发进程。目录规则见 [服务模块](../crates/wingmate-service/README.md#数据与管理边界)。

先确认示例端口空闲。若同一工作副本已有开发窗口，先关闭它，确保 WebView2 按本次参数启动。调试环境变量只设置在启动终端。

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9223'
pnpm.cmd dev --no-dev-server
```

`pnpm dev` 已通过 `beforeDevCommand` 准备 DSH 和构建首页。普通 `pnpm dev` 可检查工作台，但回到 `http://tauri.localhost/` 时可能报 `asset not found: index.html`。调试首页、停止后恢复或重试时，使用上面的 `--no-dev-server`，让本地首页资源由 Tauri 提供。

保持启动终端运行，在另一终端的同一工作副本检查后台和目标。主窗口连接后台后会切到 `status` 返回的 DSH Web 来源。检查首页时用 `http://tauri.localhost` 匹配。排除 `about:blank` 等临时授权窗口，导航后按当前来源重新确认目标。

```powershell
$state = & .\target\debug\wingmate.exe status | ConvertFrom-Json
$state | Select-Object phase, instance, engine_pid, runtime, runtime_revision, url

$pages = (Invoke-RestMethod http://127.0.0.1:9223/json) |
  Where-Object type -eq 'page'
$pages | Select-Object id, @{Name='Origin'; Expression={ ([uri]$_.url).GetLeftPart('Authority') }}
```

用 CDP 客户端连接所选目标的 `webSocketDebuggerUrl`。以下探针使用项目要求的 Node 内置 WebSocket，可直接检查页面和桥接。检查工作台时将 `$origin` 改为 `$state.url`。

```powershell
$origin = 'http://tauri.localhost'
$target = @($pages | Where-Object { ([uri]$_.url).GetLeftPart('Authority') -eq $origin })
if ($target.Count -ne 1) { throw 'Expected one matching page target' }
@'
const socket = new WebSocket(process.argv[2]);
const timeout = setTimeout(() => { throw new Error('CDP timeout'); }, 5000);
socket.addEventListener('open', () => socket.send(JSON.stringify({
  id: 1,
  method: 'Runtime.evaluate',
  params: {
    expression: `({ origin: location.origin, ready: document.readyState,
      tauri: typeof window.__TAURI_INTERNALS__?.invoke,
      home: typeof window.renderWingmateHome,
      bridge: typeof window.__WINGMATE_BACKEND__?.manage })`,
    returnByValue: true
  }
})));
socket.addEventListener('message', ({ data }) => {
  const reply = JSON.parse(data);
  if (reply.id !== 1) return;
  clearTimeout(timeout);
  socket.close();
  if (reply.error || reply.result.exceptionDetails) throw new Error('CDP evaluation failed');
  console.log(reply.result.result.value);
});
'@ | node --input-type=module - $target.webSocketDebuggerUrl
```

首页的 `home`、工作台的 `bridge` 就绪时分别为 `function`。需要验证界面操作时，通过 CDP 操作原有页面控件，再核对 DOM 和 CLI 状态。直接调用 CLI 可以验证服务行为，普通浏览器可以检查 DSH Web。Tauri 权限、桥接和窗口导航需要在实际 WebView 中复验。

## 后台与发行产物

后台问题先运行同一构建产物的 `status`。开发版日志在 `work/dev/wingmate/service.log`，发行版在当前管理目录的 `service.log`。启动或停止异常时，把日志与 CLI 返回的阶段、实例标识及 Node PID 对照。桌面宿主的启动错误看启动终端，前端异常看 CDP Console。分享日志、请求或目标列表前移除凭据和完整登录链接。

发行版特有问题使用 `pnpm build` 的产物复验。先用 `pnpm test:smoke` 检查复制出来的 EXE、后台生命周期和 DSH Web 认证。需要看真实发行窗口时，另行复制 EXE，在启动它的终端把 `WINGMATE_HOME`、`DSH_HOME`、`DSH_AGENTS_HOME` 和 `WEBVIEW2_USER_DATA_FOLDER` 分别指向本次测试的独立绝对路径，并设置空闲的 CDP 端口。对只在发行版出现的白屏，在 CDP 中记录首条异常堆栈和失败的资源请求。

## 收尾

关闭本次测试窗口和 CDP 客户端，结束本次启动命令，并确认调试端口已释放。关闭窗口后后台仍在运行。如需停止开发后台，从当前工作副本运行 `& .\target\debug\wingmate.exe stop | Out-Host`。发行测试结束后，在保持隔离环境变量的终端对复制出来的 EXE 执行 `stop`。停止后用同一个 EXE 确认 `status` 为 `stopped`。
