# GeoD 桌面消息中心集成

## 客户端

GeoD 3.6.11 顶部的消息中心采用系统浏览器 OAuth、PKCE S256 与随机 state。本机 HTTP 回调只监听 `127.0.0.1` 的随机端口。授权回调必须匹配端口、路径、Host 和 state。

注册客户端：`geod-desktop`；唯一 scope：`geod:notifications`。使用独立的系统凭据条目 `dev.geo-downloader.notifications`，不读取 GeoD Agent 的凭据。通知账号不影响原有下载能力。

服务地址默认 `https://geod.laogao.xyz`；本地集成测试可用 `GEOD_DESKTOP_IDENTITY_ORIGIN`、`GEOD_DESKTOP_GATEWAY_ORIGIN` 指向本机服务。凭据按身份服务地址隔离。本机 HTTP 被允许，远端必须 HTTPS；禁止携带账号密码、路径、查询参数或片段的服务 origin。

原生命令：`auth_status`、`auth_begin`、`auth_logout`、`notifications_list`、`notifications_read`、`notifications_open_link`。涉及网络与系统凭据的操作在后台线程执行。

## 服务端兼容补丁

身份服务的 `lib/accounts/geod-oauth.ts`、`geod-consent.ts`，以及 authorize/revoke 路由允许这个新客户端。授权码、刷新令牌和撤销必须绑定原客户端，不能从通知权限提升为 Agent 权限。保留现有 `geod-agent-desktop` / `geod:agent` 行为。

GeoD Agent 网关 `messages-store.mjs` 和 `server.mjs` 的兼容补丁新增：

- 公告 `product`：`geod-agent`（旧数据默认）、`geod`、`all`。
- `GET /api/agent/messages?product=geod&clientVersion=3.6.11`。
- `POST /api/agent/messages/read`：包含 `product: geod`、当前 `accountId`、版本和消息 ID。
- 响应包含 `product: geod`，客户端同时验证产品及账号。
- 通知 scope 只允许 GeoD 消息接口，不允许 Agent 模型、钱包或其他账户操作；读消息不调用欢迎额度发放逻辑。

继续使用现有公告 SQLite 表和管理员 JSON 发布工具，不增加公开发布接口。所有账号定向、时间、版本与已读判断均在服务端执行。`all` 公告的同一消息/修订已读记录可在两种产品中共用；产品专属公告使用各自 ID。

## 发布示例（准备稿，不能直接广播）

```json
{
  "id": "geod-desktop-3611",
  "product": "geod",
  "title": { "zh": "GeoD 3.6.11 更新", "en": "GeoD 3.6.11 update" },
  "body": { "zh": "新增消息中心并改善下载稳定性。", "en": "A new message center and download reliability improvements." },
  "priority": "normal",
  "minimumVersion": "3.6.11",
  "action": { "kind": "update", "label": { "zh": "检查更新", "en": "Check for updates" } }
}
```

须使用真实部署的数据库路径，先保存为草稿，审核内容及受众后再发布。不能把本地 QA 通知写入线上数据库。

## 本地验证与发布门槛

- 前端消息模型测试及中英文资源一致性。
- 原生回环 origin、授权 scope、Token 格式、限长回调读取和登录流程恢复测试。
- 服务端双向跨客户端换码/刷新/撤销拒绝，产品受众与账号定向隔离。
- 真实 HTTP 网关测试验证通知读取不会写入 Credits 或欢迎额度记录，不调用模型。
- 浏览器 QA 使用明确隔离的虚拟账号、模拟原生命令；它不代表真实线上 OAuth 验收。
- 安装包公开发布前先将兼容补丁合入当前线上身份/网关版本，保留生产账号和账本；回滚可以恢复旧服务版本，无表结构迁移。
- 服务端兼容更新、桌面包公开发布和公告广播分别确认结果；不能只验证本地界面便称已上线。
