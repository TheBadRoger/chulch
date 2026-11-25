# 源码目录

项目使用一个 Cargo 包。`src/main.rs` 负责进程入口和退出码，`src/lib.rs` 导出库接口。查找实现时可以从以下目录入手。

| 目录 | 职责 |
| --- | --- |
| `src/cli/` | 定义命令、分派请求、显示结果，提供交互式导入向导。`completion/` 根据同一份命令定义生成候选和说明。 |
| `src/packs/` | 读取整合包或本地启动器安装，下载依赖，解析 Minecraft 和加载器元数据。 |
| `src/instances/` | 发布、查询和卸载实例。`model.rs` 定义持久化记录，`install.rs` 在临时目录准备实例，再发布到工作区。 |
| `src/launch/` | 选择 Java、解析启动配置、准备运行目录并启动游戏。`config/` 负责 TOML 和命名参数的优先级。 |
| `src/account/` | 微软设备登录、会话刷新和本机凭据保存。 |
| `src/workspace/` | 选择工作区和当前实例，管理公共资源，检查完整性和磁盘用量。 |
| `src/storage/` | 对象读写及校验、路径检查、归档读取和文件锁。 |
| `tests/local/` | 按导入、整合包、启动、存储和命令行组织集成测试，共用的数据构造函数在 `fixture.rs`。 |

## 修改命令补全

在 `src/cli/mod.rs` 修改命令、选项及英文帮助。需要根据工作区查询候选时，修改 `src/cli/completion/mod.rs`。PowerShell 的终端接口在 `powershell.rs`，它把当前命令的语法元素和光标位置传入程序，再将返回值交给终端显示。生成候选的过程不会执行用户正在编辑的命令。

运行 `cargo test --locked` 可检查库、命令和本地集成测试；只运行某组时，例如 `cargo test --test local storage::`。
