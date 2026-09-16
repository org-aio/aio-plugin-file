# AIO 文件管理插件 / AIO File Management Plugin

文件管理作为独立 Git 插件接入 AIO IDEA 的“系统”场景，在“基础设施 → 文件管理”树节点下贡献“文件列表”页面。一个仓库内同时保存 Dioxus 客户端、Rust 服务端和共享模型。

File management plugs into the “系统” (System) scenario of AIO IDEA as an independent Git plugin, contributing the “文件列表” (File List) page under the “基础设施 → 文件管理” (Infrastructure → File Management) tree node. One repository holds the Dioxus client, Rust server and shared models together.

## 存储边界 / Storage Boundaries

- PostgreSQL 表 `file_objects` 是文件元数据的正式持久化源。
- 文件内容写入宿主配置的 `AIO_FILE_STORAGE_DIR`；目录中只使用租户摘要和随机对象 ID，不使用用户提交的文件名。
- 每次列表、下载和删除都使用登录会话中的 `tenant_id` 过滤；请求不能传入租户 ID。
- `AIO_FILE_MAX_BYTES` 控制单文件上限，默认 10 MiB。
- 文件上传先写同目录临时文件并同步，再原子切换；数据库保存失败时清理内容文件。
- 删除先把内容移入同租户隔离的回收文件，再提交元数据事务；事务失败会恢复内容。

- The PostgreSQL table `file_objects` is the authoritative persistence source for file metadata.
- File contents are written to the host-configured `AIO_FILE_STORAGE_DIR`; the directory uses only tenant hashes and random object IDs, never user-supplied filenames.
- Every list, download and delete is filtered by the `tenant_id` in the login session; requests cannot pass a tenant ID.
- `AIO_FILE_MAX_BYTES` caps the per-file size, defaulting to 10 MiB.
- Uploads first write a temp file in the same directory and sync it, then switch atomically; a failed database save cleans up the content file.
- Deletion first moves content into a tenant-isolated recycle file, then commits the metadata transaction; a failed transaction restores the content.

## API

| 方法 / Method | 路径 / Path | 说明 / Description |
| --- | --- | --- |
| `GET` | `/api/files` | 当前租户文件列表 / File list of the current tenant |
| `POST` | `/api/files?filename=<name>` | 上传原始请求体 / Upload the raw request body |
| `GET` | `/api/files/{id}` | 下载当前租户文件 / Download a file of the current tenant |
| `DELETE` | `/api/files/{id}` | 删除当前租户文件 / Delete a file of the current tenant |
| `GET` | `/api/plugins/file/health` | 初始化存储并返回健康状态 / Initialize storage and return health |

页面与 API 均要求专用的 `file:manage` 权限，文件能力不与 RBAC 管理权限耦合。

Both the page and the API require the dedicated `file:manage` permission; file capabilities are not coupled to RBAC admin permissions.

## 验证 / Verification

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# 使用独立测试 PostgreSQL 运行真实持久化与租户隔离测试
# Run real persistence and tenant-isolation tests against a dedicated test PostgreSQL
AIO_TEST_DATABASE_URL=postgresql://... \
  cargo test -p aio-plugin-file-server persists_content_and_isolates_tenants -- --ignored
```
