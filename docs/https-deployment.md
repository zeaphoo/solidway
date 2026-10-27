# 内置 HTTPS 部署与运维

> 行为规格见 `openspec/changes/2026-10-18-builtin-tls-acme/`（归档后为 `openspec/specs/platform-support` 与 `management-api`）；选型见 `docs/adr/0002-tls-acme-library.md`。
> 本文只讲部署与运维：DNS、端口、权限、单实例、staging 到生产、备份与排障。

## 0. 先决定 TLS 在哪终止

| 形态 | 关掉内置 HTTPS（默认） | 开启内置 HTTPS |
|---|---|---|
| 适用 | 前面有 nginx / Caddy / 云负载均衡 | 没有出口代理，机器直接对外 |
| 证书 | 由前置申请与续期 | 本进程用 ACME 申请与续期 |
| 监听 | 内部 HTTP（`network.bind_addr:port`） | 内部 HTTP 收缩为 loopback + `http_port` + `https_port` |
| 客户端 Base URL | `https://<前置域名>` | `https://<域名>`（证书域名） |

两者**择一**，不要叠加。叠加的后果：前置与本进程各有一份证书来源，`Secure` Cookie、HSTS、跳转判断会互相打架，排障成本翻倍。

## 1. DNS

1. 给网关准备一个公网域名，例如 `gw.example.com`；
2. 在 DNS 上把 `A`（或 `AAAA`）记录指向该机器公网 IP；
3. 把这个域名填进控制台的**对外接入地址**（`access.public_origins`，系统页），例如 `https://gw.example.com`。

证书域名**只来自对外接入地址的 host**，没有第二个配置入口。以下项会被跳过并在界面给出原因：

| 被跳过 | 原因 |
|---|---|
| `http://192.168.1.10:6565` | IP 地址（ACME 不签 IP） |
| `http://localhost:6565` | 非公网 DNS 名 |
| `https://gw.example.com:8443` | 带端口（host 必须是裸域名） |
| `https://*.example.com` | 通配符（需要 DNS-01，本版本不支持） |

多个合法域名会合并进**一张 SAN 证书**；域名增删会自动触发重新签发。

## 2. 端口、权限与防火墙

### 2.1 端口语义

- `http_port` / `https_port` 是**本机监听端口**，默认 80 / 443；
- ACME 校验固定访问公网 **80**（`http-01`）或 **443**（`tls-alpn-01`），不会访问你自定义的端口；
- 因此端口不是 80 / 443 时，必须在前面做映射，控制台会在保存后显示提示（`challenge_port_hint`）。

### 2.2 低端口权限

监听 <1024 端口需要额外权限，二选一：

~~~bash
# 方式 A：给二进制端口绑定能力（推荐，systemd 之外也有效）
sudo setcap cap_net_bind_service=+ep /usr/local/bin/solidway

# 方式 B：systemd 单元里授予
# /etc/systemd/system/solidway.service
# [Service]
# AmbientCapabilities=CAP_NET_BIND_SERVICE
# CapabilityBoundingSet=CAP_NET_BIND_SERVICE
~~~

端口绑定失败时进程会输出 `solidway: bind-failed addr=... kind=permission-denied`（或 `address-in-use`）并以非零码退出，**不会**静默降级成明文 HTTP。

### 2.3 防火墙与出站

| 方向 | 端口 | 用途 |
|---|---|---|
| 入站 | 80/tcp | ACME HTTP-01 校验 + HTTP→HTTPS 跳转 |
| 入站 | 443/tcp | HTTPS 服务（含 TLS-ALPN-01 校验） |
| 出站 | 443/tcp | 访问 ACME directory（Let's Encrypt） |

用 `tls-alpn-01` 时 80 可以不开（但要保留跳转体验，建议仍然放行）。

~~~bash
# nftables 例子
nft add rule inet filter input tcp dport { 80, 443 } accept
~~~

### 2.4 NAT / 容器里的端口映射

当本进程监听的不是 80 / 443 时（例如监听 8080 / 8443）：

- **路由器 / NAT**：把公网 80 → 内网 8080、公网 443 → 内网 8443，再做 DNS 指向；
- **Docker / Podman**：`-p 80:8080 -p 443:8443` 即可（映射发生在容器外，ACME 看到的仍是 80/443）；
- **反向代理前置**：若前面是 nginx，请改用「反代终止 TLS」形态，而不是把内置 TLS 藏在其后。

## 3. 首次开启（staging 先跑通）

1. 控制台 → 系统 → **内置 HTTPS**：填 ACME 邮箱；
2. 把 **ACME 环境**切到 `测试环境（staging）`；
3. 勾选启用、确认端口，保存；
4. 保存响应里 `restart_required=true` → 重启服务（standalone 手动重启；desktop 模式点「立即重启服务」）；
5. 启动日志出现 `内置 HTTPS 已启用`，随后可在同一区块看到证书状态从「等待签发」变为「可用」；
6. 用浏览器访问 `https://gw.example.com` —— staging 证书**不受信任**，浏览器会警告，这是预期；确认能打开即说明 DNS、端口、挑战都通了。

`staging` 的证书同样会写进缓存，**切生产前建议先删掉 staging 证书缓存**（见 §6），否则会一直用不受信任的证书直到 30 天续期窗口。

## 4. 切到生产

1. ACME 环境改为 `生产环境`；
2. 建议同时清掉 staging 缓存（`<data_root>/tls/` 下的 `*.cert.pem` / `*.meta.json`，保留 `account.json` 也可以，账户会按新环境重新注册）；
3. 保存并重启；状态变为「可用」后即可正常访问。

⚠️ 生产环境有速率限制（Let's Encrypt：同域名每周 5 张重复证书、每小时 5 次校验失败）。反复失败的代价是**整周无法签发**，所以务必先用 staging 打通。

## 5. 续期与限流

| 行为 | 规则 |
|---|---|
| 续期窗口 | 到期前 **30 天**开始 |
| 失败退避 | 5 分钟起，每次 ×2，上限 **1 小时**，带 ±20% 抖动 |
| 暂停 | 同一证书 **24 小时内失败 5 次** → `cert.state=paused`，停止自动重试 |
| 解除暂停 | 控制台点「立即重试签发」，或重启进程 |
| 热替换 | 续期成功立即生效，**不需要重启**，不断连接 |

暂停是**故意**的：自动重试会持续消耗 ACME 的失败配额。

## 6. 数据、备份与迁移

证书与账户全部落在 `<data_root>/tls/`：

~~~txt
<data_root>/tls/
├── account.json              # ACME 账户凭据（含账户私钥）
├── <digest>.cert.pem         # 证书链 + 私钥（同文件，权限 0600）
└── <digest>.meta.json        # 域名集合、到期时间、上次续期
~~~

- `<digest>` 是当前域名集合的短摘要（SHA-256 前 16 hex），**换域名就换文件名**；
- 域名集合变化时保留**最近一份未过期**的旧证书作回退，其余清理；
- 迁移机器：把整个 `tls/` 目录一起搬走即可继续用（同一域名、同一 ACME 环境）；
- 删除 `tls/` 的后果：下次启动会重新注册账户并重新签发，不算故障，但要留意速率限制；
- `account.json` 与私钥都是敏感文件，不要进版本库、不要放进共享目录（Windows 依赖目录 ACL）。

## 7. 单实例约束

同一数据目录 / 同一域名集合**只能有一个进程执行 ACME**。多实例会竞争同一个账户、同一批订单与同一批缓存文件，并快速触发速率限制。

需要高可用时：关掉内置 HTTPS，改由反向代理或外部证书分发承担 TLS。

## 8. 故障排查

| 现象 | 可能原因 | 处理 |
|---|---|---|
| `bind-failed kind=permission-denied` | 低端口无权限 | `setcap` 或 systemd `AmbientCapabilities` |
| `bind-failed kind=address-in-use` | 端口被占用（常见：系统自带 nginx） | 停掉占用者，或改端口并做映射 |
| 证书一直「等待签发」 | DNS 未指向 / 80 未放行 / 未映射端口 | 用 `curl -v http://<域名>/.well-known/acme-challenge/ping` 验证 80 是否到达本进程 |
| 状态 `paused`，`last_error` 含 `rateLimited` | 触发速率限制 | 等配额恢复（周级），期间用 staging 验证流程 |
| `last_error` 含 `dns` / `NXDOMAIN` | 域名解析失败 | 检查 A/AAAA 记录 |
| 访问 HTTP 得到 503 | 证书尚未就绪 | 属预期：未就绪时只给 ACME 挑战与 503，不跳转、不明文服务 |
| 访问 HTTP 得到 421 | 请求的 Host 不在证书域名内 | 用「对外接入地址」里的域名访问；未知 Host 故意不跳转（防开放重定向） |
| 浏览器证书域名不符 | 用的是旧缓存（域名已改） | 确认新证书已签发；`tls/` 里应有新的 `<digest>` 文件 |
| 登录后仍提示未登录 | 前置代理把 HTTPS 终止在 http 上，或混用了两种形态 | 择一：要么全部走内置 HTTPS，要么前置终止并关掉内置 HTTPS |

排查「80 是否到达本进程」的通用方法（在任何外部机器执行）：

~~~bash
curl -sv http://gw.example.com/.well-known/acme-challenge/test 2>&1 | head -20
# 期望：连到本机 80；本进程无该 token 时返回 404 "challenge not found"
# 若返回前置代理页面 / 连接超时 → DNS 或端口映射没打通
~~~

## 9. 桌面模式（`--mode desktop`）

内置 HTTPS 在桌面模式同样可用，但默认关闭，且要额外确认：

| 风险 | 说明 |
|---|---|
| 端口权限 | 普通用户无法绑定 80/443，需要提权或改端口 + 映射 |
| NAT | 大多在路由器后面，需要手工做端口映射，且公网 IP 可能变化 |
| DNS | 域名必须指向这台机器当前公网地址 |
| 单实例 | 桌面壳崩溃重启后旧进程可能仍占用端口（看门狗通常已处理） |

不满足条件时控制台只显示校验错误，不会启用。
