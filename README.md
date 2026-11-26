# chulch

[English](docs/README.en.md)

chulch 是一款面向 **Windows x64** 的第三方 Minecraft Java 版启动器和实例管理工具。它借鉴 Docker 的分层存储思路，让多个实例共享相同的游戏文件，节省磁盘空间，同时各自保留模组、配置和存档。

## 快速启动

**1. 在线安装 chulch。** 打开 PowerShell，运行以下命令。显示版本号即表示安装成功。

```powershell
& ([scriptblock]::Create((irm 'https://raw.githubusercontent.com/TheBadRoger/chulch/main/install.ps1')))
chulch --version
```

**2. 安装并启动整合包。** 将示例路径换成你下载的 Modrinth `.mrpack` 或 CurseForge 导出 ZIP。`survival` 是你给实例起的名字。

```powershell
chulch install '.\my-pack.mrpack' --id survival
chulch launch survival
```

[安装与升级](docs/install.md) · [命令参考](docs/commands.md)

chulch 由社区独立开发，是非官方 Minecraft 产品。
