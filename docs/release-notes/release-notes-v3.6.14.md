# v3.6.14 — 天地图备用 Token 与测量标签交互修复

- **多个天地图 Token 备用**：设置中保留主 Token，新增备用 Token 列表，每行一个，自动忽略空行和重复项。当前 Token 无效、无权限或额度不足时按顺序切换备用项，继续下载当前瓦片。
- **修改 Token 对同一任务生效**：保存后，正在下载的后续请求使用最新配置；恢复任务读取当前设置，修复修改 Token 后仍使用任务旧 Token 的问题。主图层、注记层和各缩放级别共用切换状态。全部 Token 不可用时保留下载进度，并提示更新 Token 后继续。
- **测量结果可置顶、可拖动**：点击测量图形或标签可将对应结果置顶；标签可以拖到空白处，并用虚线标明归属。拖动标签不改变测量边界、面积或周长。清除测量时同时删除标签和连线。

备用 Token 仅用于天地图请求，日志不显示 Token 内容。切换会遵守服务返回的等待时间；服务防护或服务故障不会被误判为 Token 额度问题。

## English

- Add an ordered list of backup Tianditu tokens. Invalid, denied or exhausted tokens fall back to the next entry and retry the same tile. Blank lines and duplicates are ignored.
- Apply saved token changes to subsequent requests in running tasks, and refresh credentials when resuming a task. Base imagery, annotation layers and zoom levels share the active token. Keep downloaded tiles when all tokens are unavailable.
- Bring measurement results forward by clicking their geometry or labels. Drag labels away from overlapping results, with a connector to their original position. Moving a label leaves geometry and measured values unchanged; clearing measurements removes labels and connectors together.

Token values are omitted from logs. Provider-wide protection and service failures do not trigger credential rotation, and server-requested delays are respected.
