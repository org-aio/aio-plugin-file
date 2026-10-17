# 文件整包验收与迁移

## 自动检查

```sh
cargo fmt --all --check
cargo clippy --locked -p aio-plugin-file-client --target wasm32-unknown-unknown -- -D warnings
cargo clippy --locked -p aio-plugin-file-server --all-targets -- -D warnings
cargo test --locked -p aio-plugin-file-client -p aio-plugin-file-model --lib
```

准备独立本机 PostgreSQL 空库 `aio_file_test`，撤销 `public` schema 对 PUBLIC 的权限，将 `AIO_FILE_TEST_DATABASE_URL` 指向它，运行：

```sh
cargo test --locked -p aio-plugin-file-server -- --include-ignored --test-threads=1
```

集成测试使用真实 PostgreSQL 限权角色、HTTP、Unix SDK 及测试专用内存对象后端，覆盖 100 MiB、空文件、摘要损坏、权限与身份头、同块重试、丢失回执、并发写入、取消、过期、墓碑和失败后重启清理。旧数据测试验证权威绑定、源文件预校验、精确元数据和 token 保留、失败续传、重复核对及冲突拒绝。缺少数据库的测试显式忽略，不能报告为通过；该对象替身不等于正式运行时和 Docker 验收。

## 静态文件数据迁移

1. 备份旧数据库、`AIO_FILE_STORAGE_DIR`、组件数据库和对象存储；停止旧文件写入。不要在业务数据库上运行测试夹具。
2. 在支持本包契约的新运行时中，为目标租户安装整包，确认 `file:manage` 权限和正式数据库绑定。复制过程需要该实例保持运行，以便使用对象 broker；迁移完成前不要把新入口交给业务用户。
3. 使用与该包同版本构建的 `dist/file-migrate`。在它能访问当前实例 `/grant`、`/broker`、`/database` 挂载的部署环境运行，提供 `AIO_PLUGIN_CONFIG` 和绝对 `AIO_PLUGIN_SOCKET`（迁移不会监听它）。不要把配置内容、令牌或数据库凭据写入日志。
4. 设置 `AIO_FILE_LEGACY_DATABASE_URL`（旧宿主数据库）、`AIO_FILE_BINDINGS_DATABASE_URL`（运行时组件资源的权威绑定库）和 `AIO_FILE_LEGACY_STORAGE_DIR`（只读挂载旧内容目录）。迁移工具核对来源、租户、schema、角色和公开入口，不接受猜测的目标 schema。
5. 执行 `file-migrate`，保存 JSON 报告。每个文件保留 ID、名称、类型、作者、微秒时间、SHA-256 和图片 token；先验证源内容，再复制分块并提交。每文件原子完成，失败后重试会核对已复制文件，不覆盖不同内容。旧表、旧正文和旧 token 不删除。
6. 图床新入口增加来源和租户路径；报告给出旧/新链接映射，需要更新外部引用。旧根路由不保留兼容代理。未知公开图片 MIME、损坏原件或身份冲突会明确拒绝迁移。
7. 旧部署组合包含 `files` 时，在新宿主停服期间执行 `aio-idea runtime migrate`，保留其他能力开关；它不替代业务数据迁移。旧租户原来停用文件管理的，切换后继续停用其整包。
8. 在新入口核对数量、摘要、图床、下载及权限后切换写入。包回滚不回滚数据库；恢复旧静态宿主前需安排新数据回迁，不能直接换回二进制。

部署者工具不包含在公开前端或自动执行路径内。实际产品、Docker、PostgreSQL 与浏览器验收需按下一节独立运行，不能以单元/集成测试推断已上线。

## 真实产品整包与浏览器

`tests/runtime/delivery.py` 使用当前 `aio-idea` 产品二进制、正式监督器、真实 Docker Sidecar 和运行时对象 broker。它拒绝非本机连接或非 `file_delivery_test` 数据库，并要求空库；不要指向业务库。先构建本包、当前宿主的 server 二进制及 Dioxus Web 产物，再构建平台 `az-plugin-bundle` 的 `package` 示例。

准备独立 PostgreSQL，撤销 `public` schema 的公共权限。在能运行宿主二进制、Python 3、Git 和 Docker CLI 的环境中设置：

```sh
export AIO_FILE_DELIVERY_HOST=/path/to/aio-idea
export AIO_FILE_DELIVERY_DATABASE_URL=postgres://postgres@127.0.0.1:5432/file_delivery_test
export AIO_FILE_DELIVERY_POSTGRES_CONTAINER=file-test-postgres
export AIO_FILE_DELIVERY_WEB=/path/to/dx/aio-idea/release/web/public
export AIO_FILE_DELIVERY_PACKAGER=/path/to/examples/package
export AIO_FILE_DELIVERY_BROWSER_CONTAINER=file-test-browser
python3 tests/runtime/delivery.py
```

浏览器容器使用 Playwright 1.62.0，开启 host network，将本仓库挂载到 `/workspace`，工作目录使用 `/workspace`。先执行 `npm ci` 并安装 Chromium，或使用相同版本的官方 Playwright 镜像。脚本通过 `docker exec` 传入测试专用随机凭据，不写入报告。省略浏览器变量只能得到后端验收，报告明确记录 `browser=false`。

在容器内运行宿主测试时，临时目录须以相同绝对路径挂载到 Docker 宿主，否则监督器无法挂载 Sidecar 的配置和 Unix socket。宿主测试进程还需访问 Docker socket；所有路径都应属于本次隔离环境。

验收覆盖：真实 100 MiB 与空文件、上传中重启后续传及不可替换分块、匿名图片与跨租户拒绝、通过真实 grant 和 broker 复制旧文件、重复迁移、升级/回滚、停用跨重启保留、实时撤权、墓碑清理、卸载和宿主二进制摘要不变。浏览器脚本运行桌面与手机尺寸，检查实际图片解码、上传取消、下载字节、剪贴板、搜索及历史、分类排序、删除确认、共享样式和横向溢出；桌面额外通过真实页面上传并下载 100 MiB。

截图与浏览器报告写入 `test-results/`，宿主脚本最后输出验收结果和临时日志目录。只清理该脚本创建的进程及专用数据库/容器；测试成功不代表生产发布或旧数据已迁移。
