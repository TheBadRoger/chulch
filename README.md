# chulch

chulch 是一个 **Windows x64** 命令行工具，用于安装和启动 Minecraft Java 版的 Fabric、Forge 和 NeoForge 整合包，支持离线角色和微软正版账号。多个实例可以共享游戏文件，各自保留模组、配置和存档。启动时会自动匹配 Java。

## 快速启动

1. **安装命令行工具。** 解压 Windows x64 安装包，在含有 `install.ps1` 的目录打开 PowerShell。如果使用源码，请先准备 Rust 1.89 或更高版本。两种方式都可按[安装指南](docs/install.md)操作。

   ```powershell
   .\install.ps1
   chulch --version
   ```

2. **安装并启动第一个整合包。** 将示例路径换成自己的 Modrinth `.mrpack` 或 CurseForge 导出 ZIP。`survival` 是给本次安装取的实例名。

   ```powershell
   chulch install '.\my-pack.mrpack' --id survival
   chulch launch survival
   ```

自定义工作区、共享资源包和光影包、快速读档与卸载等用法见[命令参考](docs/commands.md)。
