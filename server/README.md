# 文件进程插件

`hosting::run()` 从原生 SDK 读取当前实例授权，创建一个 Dill 容器并注册 `FileServiceImpl`、`FileService` 与 `FileController`。受保护路由使用可信 `Caller`；匿名图片使用仅含租户的 `PublicCaller`，两者不混用。

数据库连接只能使用宿主授权的受限角色和 `p_` schema，启动时核验角色、schema 和权限。表由整包 SQL 迁移创建，业务进程不建 schema。对象通过 `Host::storage()` 读写，运行时绑定来源与租户；文件不能访问任意宿主路径。

`service_impl.rs` 负责服务与元数据查询；`uploads.rs` 负责持久分块及提交；`streaming.rs` 负责校验和流式读取；`cleanup.rs` 负责过期及墓碑清理；`migration.rs` 负责部署者显式旧数据复制。30 秒清理循环随进程停止取消，随后关闭连接池；清理错误会记录并重试，不删除未核对的旧数据。

运行入口是 `file-server`；部署迁移工具是 `file-migrate`，共用相同领域服务和对象 SDK。完整操作与恢复边界见 [测试和迁移说明](../tests/README.md)。
