# mcdocker 命令参考

在 PowerShell 中运行 `mcdocker help` 查看所有命令，或用 `mcdocker install --help` 查看单条命令的参数。示例中的 `survival` 是实例名，`New World` 是该实例下已有的存档目录名。

| 想做什么 | 命令示例 |
| --- | --- |
| 设置默认工作区 | `mcdocker workspace use 'D:\Games\MCWorkspace'` |
| 查看实例列表 | `mcdocker list`（别名 `ls`） |
| 检查共享文件是否缺失或损坏 | `mcdocker verify` |
| 查看共享对象、各实例私有数据和运行副本的大小 | `mcdocker usage` |
| 选中常用实例 | `mcdocker use survival` |
| 查看版本、游戏目录和存档 | `mcdocker info survival` |
| 安装整合包 | `mcdocker install '.\my-pack.mrpack' --id survival` |
| 查看匹配的 Java | `mcdocker java survival` |
| 启动实例 | `mcdocker launch survival`（别名 `run`） |
| 为所有实例设置公共资源区 | `mcdocker resources use 'E:\Minecraft 公共包'` |
| 直接进入已有存档 | `mcdocker quick survival 'New World'` |
| 卸载实例并保留数据 | `mcdocker uninstall survival` |

## 工作区与当前实例

**工作区**决定从哪里读取实例和共享对象。运行 `mcdocker workspace` 或 `mcdocker workspace show` 可查看当前路径。临时查看另一工作区时，可以只给这次调用传递路径。

```powershell
mcdocker --workspace 'E:\Minecraft' list
```

工作区按以下顺序选取，先匹配的设置生效。

1. 全局选项 `--workspace`，兼容旧名 `--root`。
2. 环境变量 `MCDOCKER_WORKSPACE`。
3. `mcdocker workspace use <目录>` 保存的默认工作区。
4. 当前目录下已有的 `.mcdocker` 目录。
5. 用户目录 `%LOCALAPPDATA%\mcdocker\workspace`。

**当前实例**由各工作区分别记住。选中实例后，`info`、`java`、`launch` 和 `uninstall` 均可省略实例名。`quick` 只写存档名时，也会使用当前实例。

```powershell
mcdocker use survival
mcdocker info
mcdocker info --json
mcdocker quick 'New World'
```

## 公共资源区

把常用的资源包和光影包集中存放，当前 Windows 用户的所有 mcdocker 工作区和实例都能在启动时读取它们。首次设置会自动创建两个子目录。

```powershell
mcdocker resources use 'E:\Minecraft 公共包'
mcdocker resources show
```

| 子目录 | 放入的内容 | 在游戏中使用 |
| --- | --- | --- |
| `resourcepacks\` | 资源包 ZIP，或根目录含 `pack.mcmeta` 的包文件夹 | 打开「选项 → 资源包」选择 |
| `shaderpacks\` | 光影包 ZIP，或含 `shaders\` 的包文件夹 | 在装有兼容光影模组（如 Iris）的实例中，打开光影设置选择 |

每个包直接放在对应子目录下。用 `launch` 或 `quick` 启动实例时会读取最新内容，已经打开的实例可在下次启动时看到更改。各实例分别记住已启用的包。

如果实例已有同名包，本地包保留原名，公共包会以 `名称 (shared).zip` 等名称出现。启动期间公共包会占用实例所在磁盘的空间，游戏退出后会清理临时副本；在实例中改过的副本会作为本地包保留。

要切换公共资源区，再次运行 `resources use <目录>`。运行 `mcdocker resources clear` 可解除共享，原目录和里面的包都会保留。

## 安装整合包

```powershell
mcdocker install '.\my-pack.mrpack' --id survival
mcdocker install '.\curseforge-pack.zip' --id adventure --manual '.\补件'
```

`install` 根据压缩包根目录的清单识别分发格式。实例名应由 1 至 64 个小写英文字母、数字、连字符或下划线组成。省略 `--id` 时，从包文件名生成实例名。

| 安装选项 | 用途 |
| --- | --- |
| `--id <实例名>` | 为新实例指定名称。已有同名实例时，安装会停止。 |
| `--source <目录>` | 指向包含 `versions/`、`libraries/` 和 `assets/` 的游戏安装根目录，其中应已安装整合包要求的游戏和加载器版本。也可用 `MCDOCKER_SOURCE` 设置。 |
| `--manual <目录>` | 从本地读取手动补齐的分发包文件，目录结构见下表。 |
| `--optional` | 将分发包标记为可选的客户端文件一并装入。 |

| 格式 | 文件选择与校验 | 手动补件位置 |
| --- | --- | --- |
| **Modrinth** `.mrpack` | 按客户端环境筛选文件，核对长度、SHA-1 和 SHA-512。先应用 `overrides/`，再应用 `client-overrides/`。 | 在补件目录下保持清单中的相对路径，例如 `mods/example.jar`。 |
| **CurseForge** 导出 ZIP | 按项目 ID、文件 ID 解析模组。自动查询使用环境变量 `CURSEFORGE_API_KEY`。有 API 元数据时核对文件名、长度及支持的源哈希。 | 在补件目录下使用 `<projectID>/<fileID>/<文件名>.jar`。离线补件记录所选文件身份和本地内容哈希。 |

整合包清单决定加载器和版本。Fabric、Forge 和 NeoForge 都使用相同的 `install`、`launch` 和 `quick` 命令。`mcdocker info <实例名>` 会显示实际加载器和 Java 要求。

省略 `--source` 时，mcdocker 先查找工作区中游戏、加载器类型和版本都匹配的实例，再检查本机默认 Minecraft 安装。首次在线安装会自动优先使用中国大陆镜像。

| 下载内容 | 默认来源与校验 |
| --- | --- |
| Minecraft 客户端、资源和依赖库 | 从 Mojang 清单读取版本及 SHA-1，再通过 BMCLAPI 下载对应文件。 |
| Fabric、Forge、NeoForge | Fabric 启动清单取自官方；有上游校验值的 Maven 依赖优先使用 BMCLAPI。Forge 和 NeoForge 的安装器先核对独立发行渠道提供的 SHA-1，再执行；mcdocker 会先从镜像补齐安装清单列出的依赖，再交给安装器生成客户端。 |
| 缺少的 Temurin JRE | 从 Adoptium 查询版本与 SHA-256，优先从清华大学开源软件镜像站下载 Windows x64 ZIP。 |
| 整合包中的模组 | 有可信校验值的 Modrinth CDN 和 CurseForge `edge.forgecdn.net` 公开文件优先经 MCIM 下载，失败时回到分发包或平台提供的地址。受限文件仍可通过 `--manual` 补齐。 |

镜像可能将请求重定向到发行方。镜像缺件、下载失败或校验不通过时，启动器会尝试其他候选来源。要改用自己的兼容节点，可在当前 PowerShell 设置 `$env:MCDOCKER_MIRROR = 'https://你的镜像域名'`。该设置只覆盖 BMCLAPI 路由。要查看各次下载选中的来源，可设置 `$env:MCDOCKER_TRACE_DOWNLOADS = '1'`。[BMCLAPI 接口说明](https://bmclapidoc.bangbang93.com/) · [MCIM 接入说明](https://www.mcimirror.top/) · [清华 Adoptium 镜像说明](https://mirrors.tuna.tsinghua.edu.cn/help/Adoptium/)

如果官方源访问缓慢，可使用现有安装目录。例如，以下命令会从本机取得与整合包匹配的基础文件：

```powershell
mcdocker install '.\adventure.mrpack' --source 'E:\Games\.minecraft' --id adventure
mcdocker info adventure
mcdocker launch adventure
```

已验证的 Windows x64 客户端组合包括 Minecraft 1.20.1 搭配 Fabric 0.16.9 或 Forge 47.3.0（Java 17），以及 Minecraft 1.21.1 搭配 NeoForge 21.1.228（Java 21）。使用旧式独立 native 分类器的游戏版本暂不支持。

### 从本地 JSON 清单安装

在项目根目录执行以下命令，将路径替换为自己的游戏安装目录。示例清单固定 Minecraft 1.20.1 和 Fabric Loader 0.16.9，可复制后调整。[查看示例清单](../examples/fabric-local.json)

```powershell
mcdocker install '.\examples\fabric-local.json' --source 'E:\Games\.minecraft' --id first
```

编写其他加载器的本地清单时，将示例里的 `fabric` 字段换成 `"loader": "forge"` 和 `"loaderVersion": "47.3.0"`，并让 `loaderMetadata` 指向对应版本的 JSON。NeoForge 使用 `"loader": "neoforge"`。原有 Fabric 清单仍可直接使用。

本地清单需要通过 `--source` 或 `MCDOCKER_SOURCE` 提供源目录。清单中的源路径均相对该目录，用正斜杠分隔。示例的 `packFiles` 为空；要加入模组和初始配置，可填写以下条目，并按实际文件替换名称。每项还可提供可信的 `sha1` 和 `size`。

```json
[
  {"source": "mods/example.jar", "target": "mods/example.jar", "role": "mod"},
  {"source": "config/example.json", "target": "config/example.json", "role": "template"}
]
```

导入器只读源目录，按 Windows x64 选择库，校验并保存所需文件。缺件时可补齐后用同一实例名重试。安装完成后可移走源目录。旧命令 `mcdocker instance import <实例名> <清单> --source <目录>` 和 `mcdocker instance list` 仍可使用。

## Java 与游戏启动

```powershell
mcdocker java survival
mcdocker java survival --install
mcdocker launch survival
mcdocker launch survival --world 'New World'
```

`java` 显示匹配的版本和可执行文件路径。`java --install` 可在首次游戏启动前备好运行时。Java 的主版本由实例的 Minecraft 元数据决定。

- **自动选择。** 从工作区缓存、`JAVA_HOME`、其他 `JAVA*_HOME`、`PATH`、同目录下的其他 JDK、常见厂商目录和 Minecraft 自带运行时中查找 Windows x64 Java。主版本匹配时，优先选择较新的补丁和构建版本。
- **自动补齐。** 启动时若缺少匹配的 Java，会从 Eclipse Adoptium 获取 Temurin JRE，核对 ZIP 的 SHA-256，解包并探测版本。验证成功后存入工作区的 `runtimes/`，供多个实例复用。
- **手动指定。** `launch` 和 `quick` 均接受 `--java <java.exe 路径>`、`--player <离线玩家名>`、`--uuid <玩家 UUID>`。游戏退出后，命令返回 Java 进程的退出码。

快速读档要求游戏版本支持单人 Quick Play。可用 `mcdocker info survival` 查看存档目录名。用 `mcdocker quick survival 'New World'` 可直接进入该世界，等价于上例中的 `launch --world`。

## 核验文件与查看空间

这两条命令都检查指定工作区中的全部已安装实例。可在游戏退出后运行，得到稳定的空间统计。

```powershell
mcdocker --workspace 'E:\minecraftgames' verify
mcdocker --workspace 'E:\minecraftgames' usage
```

**`verify`** 读取实例清单，对当前版本引用的每个共享对象校验长度和 SHA-256。同一对象只读取一次。若文件缺失、内容损坏或清单无法读取，输出会列出问题和受影响的实例，并返回退出码 1。全部通过时返回 0。核验范围是安装时保存的共享内容，用户可修改的私有模组、配置和存档另行保存在实例目录。模组之间的版本兼容性由加载器在游戏启动时检查。

**`usage`** 显示文件的逻辑字节数。可加 `--json` 获取精确数值，`verify` 也支持此选项。

| 输出项 | 统计范围 |
| --- | --- |
| Object store | 当前对象库中所有文件的内容，包括已卸载实例遗留的对象 |
| Objects referenced | 当前已安装实例的清单引用的对象大小，按 SHA-256 去重 |
| Saved by sharing | 与各实例分别保留一份相同对象相比，共享减少的字节数 |
| Private game files | 按实例列出 `game/` 中的存档、配置、模组、日志等文件大小 |
| Temporary runtime copies | 采样时 `work/run-*/` 中的文件大小，会随游戏启动与退出变化 |

遍历目录时跳过符号链接和 Windows 联接点。文件系统分配空间、原始整合包、Java 缓存和卸载备份均在上述统计范围之外。

## 卸载与数据保留

先退出该实例，再执行 `mcdocker uninstall survival`。默认会从列表中移除实例，将完整实例目录移到工作区的 `removed/`，并输出备份位置。其他实例可继续使用共享对象。

> **永久删除存档与配置**  
> 只有在确定要清除该实例的私有文件时，才使用 `mcdocker uninstall survival --delete-data`。

实例的模组、设置和世界位于 `instances/<id>/game/`。启动时会为客户端、库和资源创建临时运行副本，正常退出后释放。同时启动多个实例时，应为每个实例预留副本空间。

[返回快速启动](../README.md#快速启动)
