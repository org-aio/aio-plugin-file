# 文件管理服务

服务通过 Dill 注册 Controller 与 Service。HTTP Controller 仅从身份插件读取当前用户和租户，再将命令转发给领域 Service；PostgreSQL 和文件系统实现位于 `generated/file_management/service_impl.rs`。

`FileServiceImpl` 与 `FileController` 可由宿主分别装配到同一 Dill 容器，资源初始化调用 `FileService::initialize()`，停止或初始化失败调用 `FileServiceImpl::shutdown()`。关闭只释放连接池，不删除文件或元数据；下一代使用新实例重新校验存储目录。

