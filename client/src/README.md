# 文件管理页面

沿用原 Dioxus 三栏文件浏览器与共享控件：分类、搜索、排序、文件列表、预览与元数据。上传、详情和删除确认按操作打开对话框；支持拖入文件、键盘选择和部分删除失败后的重试。

`http.rs` 通过 `window.aioPlugin` 调用当前挂载实例。文件使用 4 MiB 分块，展示进度并允许取消；取消等待当前有界请求结束。下载在点击时同步请求宿主启动，随后等待接收确认；复制等待宿主写入确认。图片仅使用服务返回的公开路径。

`view_state.rs` 以宿主 fragment 为权威，使用 `aioPlugin.navigate/onNavigationChange` 同步分类、已提交搜索和排序：`#?category=image&q=报告&sort=name`。输入草稿不入链接，回车或“搜索”一次 push；分类和排序也 push，非法值 replace，保留其他参数。列表使用 `data-url-scroll="files-list"` 接入宿主滚动恢复。勾选、上传草稿和删除确认不分享。

交付须使用当前 DX 产物，在真实 opaque iframe 中验证桌面/手机布局、图片、复制、下载、上传取消及外层链接的刷新和前后退，不能把编译成功当浏览器验收。
