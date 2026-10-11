# 经典 GeoD 公共公告

经典 GeoD 是免费开源桌面产品，消息中心不包含登录、账号管理、定向通知、钱包或 Credits 功能。

## 读取与展示

原生命令 `notifications_list` 在后台线程匿名读取 `GET /api/agent/messages/public?product=geod&clientVersion=3.6.11`。不发送 Authorization、Cookie、用户标识或设备标识，也不访问系统凭据。响应沿用公告结构，`schemaVersion: 1`、`product: geod`、`accountId: null`。

客户端提供铃铛与未读角标、全部/未读筛选、展开即已读、全部已读、重要公告提醒、离线缓存以及中英文与深浅色外观。长内容在固定最大高度的面板内滚动。

已读记录和重要提醒状态仅保存在本机，按消息 ID、修订隔离；公告修订后会再次成为未读。客户端不上传已读回执。

`notifications_open_link` 只允许不含账号密码的 HTTPS 地址；更新动作复用现有桌面更新检查。

## 公告服务

与 GeoD Agent 共用公告发布存储和管理员 CLI，但产品受众分开：`geod` 面向经典桌面版，`geod-agent` 为旧数据默认产品，`all` 为跨产品公共公告。`geod` 公告禁止填写 `audienceAccountId`。

公开接口只返回未指定账号、已发布、满足版本与时间条件的 `geod` 或 `all` 公告，不展示草稿、撤回或过期消息，也不接受客户端账号选择、已读写入或发布请求。读取时不调用身份、模型、欢迎额度或支付逻辑。GeoD Agent 原有账号功能保持独立。

发布仍由管理员保存草稿并审核内容及产品受众后进行，没有公开发布接口。公告示例：

```json
{
  "id": "geod-desktop-3611",
  "product": "geod",
  "title": { "zh": "GeoD 3.6.11 更新", "en": "GeoD 3.6.11 update" },
  "body": { "zh": "新增公共公告并改善下载稳定性。", "en": "Public notices and download reliability improvements." },
  "priority": "normal",
  "minimumVersion": "3.6.11",
  "action": { "kind": "update", "label": { "zh": "检查更新", "en": "Check for updates" } }
}
```

## 本地验证与发布

测试覆盖公共接口匿名访问、定向消息隔离、禁止 GeoD 定向发布、版本与日期过滤、无身份/模型/额度调用，以及客户端本机已读持久化和无登录控件。

本地预览只使用明确标注的示例公告与本机数据库，不发布线上公告。正式发布前需先上线公共公告兼容接口并验证真实匿名读取；无需身份服务改动或 OAuth 客户端注册。
