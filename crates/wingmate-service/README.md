# wingmate-service

独立常驻 Rust 管理服务，管理已经安装的 DSH 运行内容。业务运行资源与组件安装、版本协调、命令注册归[发行入口](../wingmate-launcher/README.md)；服务不内嵌资源归档，不依赖 GUI 或开发仓库。

## 服务管理边界

- 管理服务与 DSH 业务后台有各自的运行锁、身份和发现信息。关闭窗口不影响两者；停止业务后，服务仍可使用已安装的运行内容再次启动业务。
- 本机管理接口使用动态回环端口，仅供同一用户调用。发行入口通过 `host.json`、`host.lock` 和经认证的 `POST /rpc` 协调启停与服务切换。Web 管理请求使用配对会话和业务实例校验，直接调用同一进程内的管理逻辑。
- 只有身份确认的 DSH 进程在管理范围内。显式停止走 DSH 关闭钩子；Windows 的业务运行锁和 Job Object 仍由 Node 持有，Node 退出时回收业务子进程。管理服务意外退出不自动终止业务，恢复服务后重新确认状态。
- 停止、重启可绑定业务实例，服务取得操作锁后再次核对；旧客户端不能停止后来创建的实例。退出服务要求业务已停止。
- 服务读取发行入口发布的 `runtime.json`，启动前核对安装清单、指纹、目录归属与入口。DSH 运行内容缺失或损坏时报告修复需求，交由发行入口处理。

## Web 入口

服务提供本机、局域网和公网 Web 入口。配对后可以打开完整 DSH Web，查看状态、停止或重启当前主机的 DSH。业务停止后，同一地址提供[共享启动页](../../apps/launcher/README.md)，可启动已安装的运行内容。停止和重启绑定业务实例，并与本机启停共用操作锁。Web 重启使用已发布的运行配置，发行组件更新由[发行入口](../wingmate-launcher/README.md)执行。

连接地址可收藏。一次性配对链接默认五分钟内有效，可兑换为三十天的会话 Cookie。为同一入口重新生成链接会使该入口的上一条链接失效。配对码及会话分别绑定入口，公网还绑定对应 HTTPS 地址。撤销设备会关闭对应长连接。DSH 登录凭据留在网关内，本机管理 RPC 使用独立凭据。

首次连接可为每台设备分别生成一次性配对链接，局域网也可使用主机已启用的固定密码。客户端持有有效会话时，可从原地址继续访问；重新打开已使用的配对链接也会跳转到工作台入口。

本机桌面和通过网关访问的工作台都使用 DSH 自带的网页目录选择器。「添加工作区」时可浏览和选择运行 DSH 的主机上的目录。

正式版网关初次按 `8746`、`8747`、`8748` 的顺序选择一个可用端口，开发版用 `8749`。本机 Web、已启用的局域网地址和公网隧道回源使用选中的同一端口。管理服务重启和增加局域网监听时优先复用上次使用的端口，只有地址冲突时才尝试其他候选。切换端口会更新连接地址并重启已启用的隧道。端口全部占用时，可通过 `wingmate share-status` 查看绑定错误。DSH 本身使用独立的动态回环端口。

桌面和 Android 通过网关的 `GET /_wingmate` 确认入口类型。网关在线即可发现主机，工作台和恢复页都需要有效会话。连接收藏的局域网地址时，客户端先尝试记录的端口，再探测同一 IP 的其他候选端口。换端口后如果需要认证，重新登录成功时会更新收藏地址。

### 本机 Web

本机入口仅监听 `127.0.0.1`，局域网分享关闭时也可用。桌面通过本机管理接口领取一次性凭据打开工作台。通过桌面连接浮窗或 `wingmate share-pair-local` 生成供本机浏览器使用的配对链接。

### 局域网分享

分享默认关闭。默认开启时，服务在已启用网卡上的私网 IPv4（RFC 1918）使用同一端口。局域网地址优先返回系统默认路由对应的地址，每个地址都有各自的配对链接。同一次生成的局域网链接共用一个配对码，可兑换一次。局域网使用 HTTP，适用于可信网络。关闭分享会断开局域网连接，本机 Web 和已开始的业务任务继续运行。

局域网密码默认未设置。只在 LAN 入口校验带盐的 Argon2id 哈希，并对登录尝试限速。密码登录兑换同样可撤销的设备会话。修改或停用密码控制后续登录，已发出的会话通过设备列表撤销。桌面设置时另将密码保存到当前用户的系统凭据库，通过本机连接浮窗按需查看。无界面主机可运行 `wingmate share-password-set` 从标准输入设置只供校验的密码，或运行 `wingmate share-password-disable` 停用。

管理服务恢复时重新绑定已启用的分享地址。若指定网卡地址已变化，在主机上重新开启分享。

无界面主机可先运行 `wingmate service-start`，再使用 `wingmate share-enable [IPv4]`、`wingmate share-pair`、`wingmate share-status`、`wingmate share-revoke <设备 ID>` 和 `wingmate share-disable`。为 `share-enable` 指定 IPv4 时只监听该地址。首次准备业务运行内容使用 `wingmate start`。

### 公网隧道

公网分享默认关闭，与局域网独立。每台管理服务运行一条 Cloudflare 隧道。临时模式由 Cloudflare 分配 `*.trycloudflare.com`，连接器重启后从新地址重新配对。具名模式使用已配置的 HTTPS 域名，Cloudflare 发布路由的 Service URL 填桌面或 `wingmate tunnel-status` 显示的实际回源地址，保留公开域名作为请求 Host。网关换到备用端口后，同步修改 Cloudflare 的 Service URL。

配置具名隧道时保存域名和 Tunnel Token。Token 留在主机管理目录内。保存配置后，在下次显式启动或重启隧道时应用。管理服务重启会恢复上次开启的模式。停止 DSH 时隧道继续提供恢复页。关闭隧道会断开公网连接。具名模式再次开启同一域名时，未过期或撤销的配对继续有效。

无 GUI 时，可运行 `wingmate tunnel-configure <域名>` 并通过标准输入传入 Token。用 `wingmate tunnel-start quick` 或 `wingmate tunnel-start named` 启动或切换，再用 `wingmate tunnel-status` 查看状态。连接器就绪后，`wingmate tunnel-pair` 生成 `https://域名/?auth=<配对码>`。关闭用 `wingmate tunnel-stop`，撤销设备用 `wingmate share-revoke <设备 ID>`。

具名隧道可为一次性邀请选择 5 分钟或 24 小时有效期，CLI 使用 `wingmate tunnel-pair [5m|24h]`。未使用的具名邀请在后台重启后仍可兑换，重新生成或用 `wingmate tunnel-revoke-invite` 可以撤销旧邀请。临时隧道使用 5 分钟有效期。

首次开启会查找 `CLOUDFLARED_PATH`、系统 PATH 和管理目录下的 `tunnel/bin/`。缺少可用程序时，使用系统 Node 按平台下载官方连接器，校验 SHA-256 和版本输出后保存到该目录，供两种模式复用。自动下载覆盖 Windows x64、macOS x64/ARM64、Linux x64/ARM64，其他平台可手动提供可执行文件。

[`src/tunnel/download.mjs`](src/tunnel/download.mjs) 中的下载逻辑移植自 deepseek-harness-desktop `fa55bd68883e`，适用上游 MIT 文本及禁止商业二次开发的附加条款。[MIT 文本](src/tunnel/LICENSE.desktop) · [上游附加条款](https://github.com/dsh-tauri/deepseek-harness-desktop/blob/fa55bd68883e/LICENSE.details)

## 数据与管理边界

发行版默认管理目录为 `~/.wingmate`，`WINGMATE_HOME` 可指定另一个绝对路径。服务发现信息在 `host.json`，内部业务发现信息在 `endpoint.json`，两者均不是跨用户或远程信任机制。管理凭据和 Web 登录 token 不进入普通 CLI 状态。

开发入口固定使用当前工作副本的 `work/dev/wingmate`，为 DSH 设置 `work/dev/dsh` 与 `work/dev/agents`。发行版 DSH 会话、凭据与设置沿用官方目录，不能与可重新释放的管理内容一起回收。

业务安装和启停共用操作锁。业务日志为 `service.log`，独立管理服务日志为 `host.log`。释放、升级和回收规则见[运行时维护指南](../../docs/runtime-maintenance.md)。
