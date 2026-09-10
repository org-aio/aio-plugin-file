# 文件管理服务

服务通过 Dill 注册 Controller 与 Service。HTTP Controller 仅从身份插件读取当前用户和租户，再将命令转发给领域 Service；PostgreSQL 和文件系统实现位于 `generated/file_management/service_impl.rs`。

