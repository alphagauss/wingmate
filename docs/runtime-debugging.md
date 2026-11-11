# 运行时调试

先用源码、静态检查和相关测试缩小范围。需要运行时证据时，明确复现条件、需要观察的状态和复验标准。按 [验证约定](../scripts/README.md) 隔离测试数据。

| 排查对象 | 首选证据 |
| --- | --- |
| 后台启动、身份、复用和退出 | CLI 的状态和退出码、`service-status`、`service.log`、`host.log` 及各自 PID |
| 首页、DSH 工作台、后台服务设置和 Tauri 桥接 | 真实 Tauri WebView2 的 CDP。检查 DOM、Console、Network，按需在页面内操作并复查状态 |
| 系统对话框、窗口焦点和默认浏览器交接 | 先查相关测试、原生日志和进程证据。实际外观和焦点优先请用户人工复验 |

只有确实需要原生界面证据，且上述手段与人工反馈都无法提供时，才使用 Computer Use。先说明缺少什么证据，只执行必需的原生步骤。CDP 连不上时先查启动输出、监听端口、WebView2 进程和数据目录。

## 本地开发与构建

日常开发和运行验证从当前工作副本根目录运行 `pnpm dev`。它自动准备开发运行时，复用 `target/debug` 的构建产物和 `work/dev` 的运行时安装树。首次启动或组件变更时会准备相应内容，无需手动复制发行 EXE 或为每次测试另建运行时。开发后台、DSH 数据和 WebView 资料都在 `work/dev` 下，与发行版隔离。目录规则见[服务模块](../crates/wingmate-service/README.md#数据与管理边界)。

桌面首页和连接浮窗使用 Tauri 内置开发服务器。保存对应前端源码会同步到开发资源目录，打开的页面随服务器通知刷新。桌面 Rust 源码由 Tauri 监听并重新编译。

`pnpm dev` 监听 `plugins/` 下各插件的源码，以及 `packages/` 中的共享源码。声明 `dsh.client` 和 `./client` 导出的插件自动参与构建，优先执行 `build:client`，否则使用 `build`。生成目录如 `dist`、`lib` 及依赖、测试目录不参与监听。构建成功且客户端声明兼容时，开发工具发布已安装插件的客户端入口，由 DSH 热重载该插件。

独立的 `wingmate-service`、发行入口、插件宿主与独立资源，以及 `runtime/` 和 `plugins/cordis.patch.yml` 的变更需要重新准备运行时。修改这些内容后重新执行 `pnpm dev`，通过「重启并更新」切换已有后台。共享启动页的桌面版本可直接刷新，网关版本随管理服务更新。

确需编译时，dev 构建使用 `target/debug`，发行构建使用 `target/release`。沿用 Cargo 的默认目录和增量缓存，避免通过 `CARGO_TARGET_DIR` 或 `--target-dir` 新建目录，引发全量编译并增加耗时和磁盘占用。

如果正在运行的应用占用了待更新的构建产物，以开发为优先。确认占用的进程后，可以直接关闭对应应用，再在原目录继续构建。调试需要让新启动参数生效时，也可以直接关闭当前工作副本已有的 dev 窗口。

关闭窗口后后台仍在运行。如需停止 dev 后台，在当前工作副本根目录运行 `pnpm dev:stop`，依次停止业务和管理服务。

## Windows 开发窗口

启动前确认示例端口空闲。调试环境变量只设置在启动终端。

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9223'
pnpm.cmd dev
```

保持启动终端运行，在另一终端的同一工作副本检查后台和目标。主窗口连接后台后会切到 `status` 返回的 DSH Web 来源。首页和浮窗使用启动日志中的开发服务器地址，默认端口为 `1430`。排除 `about:blank` 等临时授权窗口，导航后按当前来源重新确认目标。

```powershell
$state = & .\target\debug\wingmate.exe status | ConvertFrom-Json
$state | Select-Object phase, instance, engine_pid, runtime, runtime_revision, url

$pages = (Invoke-RestMethod http://127.0.0.1:9223/json) |
  Where-Object type -eq 'page'
$pages | Select-Object id, @{Name='Origin'; Expression={ ([uri]$_.url).GetLeftPart('Authority') }}
```

用 CDP 客户端连接所选目标的 `webSocketDebuggerUrl`。以下探针使用项目要求的 Node 内置 WebSocket，可直接检查页面和桥接。检查工作台时将 `$origin` 改为 `$state.url`。

```powershell
$origin = 'http://127.0.0.1:1430'
$target = @($pages | Where-Object { ([uri]$_.url).GetLeftPart('Authority') -eq $origin -and ([uri]$_.url).AbsolutePath -eq '/' })
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

后台问题先运行同一构建产物的 `status` 和 `service-status`，区分业务停止与管理服务退出。开发版日志在 `work/dev/wingmate/service.log`，发行版在当前管理目录的 `service.log`。启动或停止异常时，把日志与 CLI 返回的阶段、实例标识及 Node PID 对照。桌面宿主的启动错误看启动终端，前端异常看 CDP Console。分享日志、请求或目标列表前移除凭据和完整登录链接。

符合[发行版验证条件](../scripts/README.md#验证边界)时，使用 `pnpm build` 的产物复验。先用 `pnpm test:smoke` 检查复制出来的 EXE、后台生命周期和 DSH Web 认证。需要检查真实发行窗口时，在 `work/tests` 下创建本次专用目录，把复制出的 EXE、调试脚本、浏览器资料和 `WINGMATE_HOME`、`DSH_HOME`、`DSH_AGENTS_HOME`、`WEBVIEW2_USER_DATA_FOLDER` 对应的绝对路径都放在该目录内。设置空闲的 CDP 端口。对只在发行版出现的白屏，在 CDP 中记录首条异常堆栈和失败的资源请求。

## 收尾

关闭本次测试窗口、浏览器和 CDP 客户端，结束本次启动命令，并确认调试端口已释放。开发后台停机见[本地开发与构建](#本地开发与构建)。发行测试结束后，在保持隔离环境变量的终端对复制出来的 EXE 执行 `stop`。用同一个 EXE 确认 `status` 为 `stopped`，再执行 `service-stop` 并确认 `service-status` 为 `stopped`。

记录需要保留的验证结论后，确认测试专用目录的实际路径和归属，再删除整个目录，包括 EXE 副本、释放的运行时、测试数据、浏览器资料、日志和临时脚本。失败现场可以保留到排查结束，随后同样清理。`work/dev`、`target`、构建缓存和日常用户数据属于可复用或用户目录，按各自规则维护。
