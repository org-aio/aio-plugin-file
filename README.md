# AIO 文件管理插件

文件管理以 v2 Sidecar 整包交付，包含 Rust 服务、Dioxus 页面和 SQL 迁移。安装后动态贡献“系统 → 基础设施 → 文件管理 → 文件列表”，升级、回滚或停用均不要求重编宿主。正式数据使用插件独立 PostgreSQL schema 和运行时对象存储；没有宿主本地文件目录或全局数据库连接的备用实现。

页面与业务 API 要求租户内的 `file:manage` 权限。安装不会授予该权限；身份、挂载、版本和撤权由宿主校验。匿名图床只开放整包声明的图片 GET/HEAD 路由，不开放列表或修改。

## 文件与图床

- 单文件最多 **100 MiB**，按 4 MiB 分块传输；普通文件、图片和空文件均可上传。
- 上传会话与分块摘要持久化，重试必须携带相同内容。全部分块验证通过后才进入文件列表，完成操作幂等。
- 页面最多重试同一分块三次；取消在当前有界请求结束后生效，停止后续分块并撤销会话。关闭页面造成的孤立上传 24 小时后过期。
- 私有下载与公开图片逐块读取，检查大小和摘要，不通过 iframe 消息缓存整个文件。下载按钮由宿主发起浏览器下载。
- 删除立即隐藏文件与图片，再通过持久化墓碑清理对象；60 秒宽限避免迟到写入留下孤立对象。失败保留状态，重启后重试。
- 图床地址包含插件来源、租户及随机 token：`/api/runtime/components/public/{source}/{tenant}/i/{token}`。普通文件没有公开地址；公开资源不缓存，删除或停用立即影响后续访问。
- 包回滚只恢复代码和前端，不回滚业务数据。

## 业务路由

以下路径通过宿主挂载和可信入口调用，不是站点根路径。

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/files` | 当前租户文件及容量限制 |
| GET / DELETE | `/files/{id}` | 私有流式下载 / 删除 |
| POST | `/uploads` | 声明名称、MIME 和字节数，创建会话 |
| GET / DELETE | `/uploads/{id}` | 当前用户的分块进度 / 取消 |
| PUT | `/uploads/{id}/chunks/{index}` | 上传固定大小的原始分块 |
| POST | `/uploads/{id}/complete` | 验证完整内容并提交 |
| GET / HEAD | `/i/{token}` | 经宿主公开入口读取图片 |

`/health` 和 `/aio/describe` 只用于宿主健康检查和动态页面发现。旧 `/api/files`、站点根 `/i/{token}` 和旧静态注册接口不保留别名。旧链接迁移输出映射，见迁移说明。

## 构建与验收

使用仓库固定的 Rust nightly、Dioxus CLI 0.7.9 和 Node。执行 `sh scripts/build.sh --process` 生成 `dist/file-server`、`dist/frontend` 及部署者工具 `dist/file-migrate`；进程构建还需 cargo-zigbuild 和 Zig。整包只包含运行时服务、前端及迁移 SQL；`file-migrate` 是单独保存和校验的部署工具，不能放入公开前端目录。

以当前平台的 `aio plugin validate` / `aio plugin package` 校验和打包；宿主须支持 Sidecar 对象 broker、公开资源流和按插件限定的公开图片预览。安装包需先在隔离环境验收，源码 push 不代表已部署。

建议阅读顺序：`server/src/hosting.rs` 的资源与路由装配 → `server/src/generated/file_management/` 的上传、读取、删除和迁移 → `client/src/http.rs` 的宿主 SDK 与 `client/src/page.rs` 的页面。测试、迁移和实际产品验收见 [tests/README.md](tests/README.md)。
