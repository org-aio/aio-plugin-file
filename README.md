# AIO 文件管理插件

文件管理作为独立 Git 插件接入 AIO IDEA 的“系统”场景，在“基础设施 → 文件管理”树节点下贡献“文件列表”页面。一个仓库内同时保存 Dioxus 客户端、Rust 服务端和共享模型。

## 存储边界

- PostgreSQL 表 `file_objects` 是文件元数据的正式持久化源。
- 文件内容写入宿主配置的 `AIO_FILE_STORAGE_DIR`；目录中只使用租户摘要和随机对象 ID，不使用用户提交的文件名。
- 每次列表、下载和删除都使用登录会话中的 `tenant_id` 过滤；请求不能传入租户 ID。
- `AIO_FILE_MAX_BYTES` 控制单文件上限，默认 10 MiB。
- 文件上传先写同目录临时文件并同步，再原子切换；数据库保存失败时清理内容文件。
- 删除先把内容移入同租户隔离的回收文件，再提交元数据事务；事务失败会恢复内容。

## API

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/api/files` | 当前租户文件列表 |
| `POST` | `/api/files?filename=<name>` | 上传原始请求体 |
| `GET` | `/api/files/{id}` | 下载当前租户文件 |
| `DELETE` | `/api/files/{id}` | 删除当前租户文件 |
| `GET` | `/api/plugins/file/health` | 初始化存储并返回健康状态 |

页面与 API 当前要求 `rbac:manage` 或专用的 `file:manage` 权限。页面仍使用 `rbac:manage` 作为壳层可见性条件，便于现有租户管理员直接使用。

## 验证

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# 使用独立测试 PostgreSQL 运行真实持久化与租户隔离测试
AIO_TEST_DATABASE_URL=postgresql://... \
  cargo test -p aio-plugin-file-server persists_content_and_isolates_tenants -- --ignored
```
