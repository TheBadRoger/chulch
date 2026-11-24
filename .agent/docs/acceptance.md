# 测试与验收

## 2026-11-24 国内镜像与公开整合包实测

在 Windows x64 上以 `E:\minecraftgames` 为工作区，从 Modrinth 下载作者发布的原始 `.mrpack`，按发布记录校验 SHA-512。通过已安装的 `mcdocker --workspace E:\minecraftgames install` 安装，启动时由 mcdocker 选择 Java。下表的模组数统计实例私有 `game/mods` 目录中的 JAR。

| 整合包（Modrinth 版本 ID） | Minecraft | 加载器 | Java | 模组数 | 客户端验收 |
| --- | --- | --- | ---: | ---: | --- |
| Simply NeoForged 1.21.1-1.0.1 (`5e0i2ymF`) | 1.21.1 | NeoForge 21.1.168 | 21 | 12 | 创建方块纹理图集，关闭游戏窗口后退出码为 0 |
| Optimized FPS 2.4.10 (`bf2aWUzR`) | 1.20.1 | Forge 47.4.3 | 17 | 25 | 创建方块纹理图集，关闭游戏窗口后退出码为 0 |
| Simply Optimized 1.20.1-12.3 (`MSICaFbr`) | 1.20.1 | Fabric 0.16.0 | 17 | 18 | 两次启动均创建方块纹理图集并正常退出，第二次记录到游戏窗口 |
| Simply Optimized & Up to Date 1.20.1-1.0.0 (`W2X8ij3C`) | 1.20.1 | Fabric 0.18.4 | 17 | 17 | 安装成功；Fabric 在渲染前检测到模组不兼容，退出码为 1 |

**失败实例** `simply-optimized-up-to-date` 保留了作者发布的模组组合。游戏日志报告 Nvidium 0.2.6-beta 声明仅兼容 Sodium 0.5.7 或 0.5.8，实际随包装入的是 0.5.13。关闭 Fabric 的冲突提示后，`mcdocker launch` 返回了游戏的退出码 1。

原始安装包保存在工作区的 `packs\`，安装日志与客户端输出保存在 `verification\`，游戏日志位于 `instances\<实例 ID>\game\logs\`。Optimized FPS 和 Simply Optimized & Up to Date 的安装日志分别记录了 22 次和 17 次经 MCIM 成功下载，游戏资源走 BMCLAPI。另行验证了从清华 TUNA 下载 Temurin JRE、核对 Adoptium 发布的 SHA-256，以及运行所下载的 Java。

### 四个实例的共享对象核验

从四份 `profile.json` 出发，读取当前代、层清单及其依赖。在每个实例内先按 SHA-256 去重，再对整个工作区合并同内容对象。

| 对象内容统计口径 | 字节数 |
| --- | ---: |
| 四个实例各存一份，各实例内部去重 | 3,270,478,932 |
| 四个实例共用对象库，跨实例去重 | 1,142,858,338 |
| 因跨实例共享而减少的对象内容 | 2,127,620,594 |

按清单逐个读取了全部 4,272 个被引用的对象，文件长度和 SHA-256 均匹配。这次统计仅覆盖被引用对象的内容字节。评估整机磁盘收益还需计入私有游戏数据、文件系统分配空间和运行时临时副本。

随后使用新增的 `mcdocker verify --json` 和 `mcdocker usage --json` 在保留的工作区复测。核验返回 4 个实例、4,272 个对象、0 个问题，空间统计复现了上表中的对象库大小和节省量。此次四个私有游戏目录合计 101,700,254 字节，客户端均已退出，运行副本为 0 字节。离线回归测试在临时工作区制造了对象缺失、同长度内容损坏和不可读的实例清单，验证了受影响实例的归属和错误退出码。

## 运行测试

运行 `cargo test --locked` 可验证并发导入、去重、私有文件隔离、完整性校验和非法路径处理。

Windows 上将 `MCDOCKER_TEST_JAVA` 指向 64 位 Java 17 的可执行文件，再运行 `cargo test --locked failed_java -- --ignored` 可验证 JVM 失败后的锁释放与存档保留。要验收全新工作区的官方下载，将 `MCDOCKER_TEST_PACK` 指向 Fabric 分发包的绝对路径，再运行 `cargo test --locked installs_public_pack -- --ignored --nocapture`。

## 命令行与分发包验收记录

从项目目录之外直接调用已安装的 `mcdocker`，成功列出既有实例并取得 Java 路径。本机 PATH 首选 Java 25，启动器为要求 Java 17 的实例选中了本地 Java 17。独立测试从 Adoptium 下载 Windows x64 JRE，核对 SHA-256，解包并成功运行版本探测。

在空工作区安装包含 FerriteCore 6.0.1 的验收用 `.mrpack`，从公开源取得 Minecraft 1.20.1、Fabric Loader 0.16.9、3,577 个资源对象和模组，完成校验及实例发布。另用离线夹具验证了 CurseForge 按项目与文件 ID 手动补件、Modrinth 客户端覆盖顺序、工作区跨进程选择、运行锁和两种卸载模式。CurseForge 持密钥的真实 API 下载仍待实测。

## 现代加载器的统一启动验收

从本机已备妥的游戏目录导入 Forge 和 NeoForge 的 Modrinth 测试包，并从原有工作区复用 Fabric 启动清单创建独立测试实例。三个实例均经普通 `mcdocker launch` 启动，记录游戏窗口、方块纹理图集创建和正常退出。测试包仅声明加载器，验收范围为客户端启动和渲染。

| Minecraft | 加载器 | 自动选择的 Java | 退出码 |
| --- | --- | --- | --- |
| 1.20.1 | Fabric 0.16.9（旧清单） | Windows x64 Java 17 | 0 |
| 1.20.1 | Forge 47.3.0 | Windows x64 Java 17 | 0 |
| 1.21.1 | NeoForge 21.1.228 | Windows x64 Java 21 | 0 |

两组 BootstrapLauncher 客户端都需要运行库目录中的 `client-…-srg.jar` 和 `client-…-extra.jar`。前者缺失时，Forge 能创建早期窗口，但在启动客户端时报告找不到 Minecraft 类。回归测试还覆盖两种分发格式、相同版本号的不同加载器、加载器参数和跨实例复用。

NeoForge 官方安装器的生成测试可单独运行。将 `MCDOCKER_TEST_SOURCE` 设为含 Minecraft 1.21.1 原版的安装目录，运行 `cargo test --locked --lib official_neoforge_installer -- --ignored --nocapture`。早期直连 `maven.neoforged.net` 时在获取校验值处超时。切换国内镜像后，21.1.228 安装器和依赖下载测试通过，官方处理器生成了启动所需的全部客户端文件。上表记录的是使用本地源的真实客户端启动。

Forge 47.3.0 另从空工作区下载了 Mojang 游戏文件和官方安装器。官方安装器成功生成客户端补丁。下载 3,577 个资源对象时，三个大文件在官方源重试后断开。保留已下载的文件，经 BMCLAPI 补齐缺项并核对官方 SHA-1，再用这份源目录执行 `install`。安装后的客户端完成渲染，退出码为 0。单次仅用官方源的完整安装未通过。

## 第一阶段 Fabric 离线客户端验收记录

本机以 Minecraft 1.20.1、Fabric Loader 0.16.9、Windows x64 OpenJDK 17.0.0.1 和空的 `packFiles` 完成实际客户端验收。两个实例分别创建并保存名为 `New World` 的生存世界。退出 JVM 后分别通过 `--world 'New World'` 重进，两份日志都记录了玩家进入世界以及所有维度保存完成。

| 实例 | 初次进入坐标 | 快速读档坐标 |
| --- | --- | --- |
| first | −8.5, 71.0, 9.5 | −8.5, 71.0, 9.5 |
| second | −5.5, 65.0, −0.5 | −5.5, 65.0, −0.5 |

两个实例引用同一 generation。第二次导入后对象库仍有 3,631 个对象，内容合计 726,670,064 字节。两个客户端退出后，运行暂存目录为空，逐对象复算 SHA-256 全部匹配。该字节数只统计对象内容，文件系统分配空间和游玩峰值另行测量。
