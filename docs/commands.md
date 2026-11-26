# chulch 命令参考

下面用 `survival` 表示实例名，用 `New World` 表示存档名。运行 `chulch help` 查看全部命令，或用 `chulch launch --help` 查看某条命令的选项。

## 安装、导入与管理实例

| 想做什么 | PowerShell 命令 |
| --- | --- |
| 设置默认工作区 | `chulch workspace use 'E:\minecraftgames'` |
| 查看当前工作区 | `chulch workspace show` |
| 安装 Modrinth 或 CurseForge 整合包 | `chulch install '.\my-pack.mrpack' --id survival` |
| 从已有客户端导入实例 | `chulch import 'D:\Games\.minecraft'` |
| 查看实例列表和详细信息 | `chulch list` 或 `chulch list -l` |
| 查看一个实例的版本、游戏目录和存档名 | `chulch info survival` |
| 选为当前实例，后续启动时可省略实例名 | `chulch use survival` |
| 卸载实例并保留数据备份 | `chulch uninstall survival` |

导入时按提示选择版本，以及要复制的存档、配置、资源包和光影包。安装 CurseForge 导出 ZIP 时，如果提示缺少文件，按提示下载，再用 `--manual 'D:\补件'` 指定补件目录。

> **永久删除**：`chulch uninstall survival --delete-data` 会同时删除该实例的存档和配置。

## 启动与账号

| 想做什么 | PowerShell 命令 |
| --- | --- |
| 启动实例（默认使用离线角色） | `chulch launch survival` |
| 使用指定离线角色名 | `chulch launch survival --player Alex` |
| 直接进入存档（需要游戏版本支持快速读档） | `chulch quick survival 'New World'` |
| 登录微软账号，按提示在浏览器中完成授权 | `chulch account login --client-id <应用客户端ID>` |
| 使用已登录的正版账号启动 | `chulch launch survival --account microsoft` |
| 查看或退出已登录的账号 | `chulch account status` 或 `chulch account logout` |
| 查看匹配的 Java，缺少时下载安装 | `chulch java survival` 或 `chulch java survival --install` |
| 用编辑器修改默认内存、窗口大小等选项 | `chulch config survival` |
| 清空实例的默认启动参数 | `chulch args clear survival` |

只想为这次启动传入参数时，可运行 `chulch launch survival --jvm-arg=-Xmx4G -- --fullscreen`。如果在 `config` 中已填写同一项常用设置，该设置优先。

## 公共资源与维护

运行 `chulch resources use 'E:\minecraftgames\common'`，把资源包放进其中的 `resourcepacks`，光影包放进 `shaderpacks`。下次启动任意实例后，即可在游戏里选择对应的包。使用光影需要实例支持光影。

| 想做什么 | PowerShell 命令 |
| --- | --- |
| 查看公共资源区或解除共享 | `chulch resources show` 或 `chulch resources clear` |
| 检查游戏文件是否缺失或损坏 | `chulch verify` |
| 查看磁盘占用和共享文件节省的空间 | `chulch usage` |

## 命令补全

在 PowerShell 中运行以下命令，然后按 **Tab** 补全命令、选项或实例名。要在每次打开终端时启用，将同一行加入 `$PROFILE`。

```powershell
chulch completions powershell | Out-String | Invoke-Expression
```

[返回快速启动](../README.md#快速启动) · [安装与升级](install.md)
