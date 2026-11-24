# 在 Windows 上安装 mcdocker

使用 **Windows x64**，在普通用户的 PowerShell 中安装。默认位置是 `%LOCALAPPDATA%\Programs\mcdocker\bin`。安装脚本会将这个目录加入当前终端和当前用户的 `PATH`。Java 会在首次启动游戏时按实例要求匹配。

## 使用 Windows x64 安装包

1. 将安装包中的文件**全部解压**到一个文件夹。
2. 在能同时看到 `install.ps1` 和 `mcdocker.exe` 的目录打开 PowerShell，运行：

   ```powershell
   .\install.ps1
   mcdocker --version
   ```

看到类似 `mcdocker 0.1.0` 的版本号就完成了安装。使用预编译包可以跳过 Rust 工具链的安装。

## 从项目源码安装

1. 安装 **Windows x64 Rust 1.89 或更高版本**，包含 Cargo。如使用默认 MSVC 工具链，按 rustup 提示安装 Visual Studio 的 C++ 桌面开发工具和 Windows SDK。[Rust 安装入口](https://rust-lang.org/tools/install/) · [MSVC 环境说明](https://rust-lang.github.io/rustup/installation/windows-msvc.html)
2. 重新打开 PowerShell，进入含有 `Cargo.toml` 的项目根目录，执行：

   ```powershell
   rustc --version
   .\install.ps1
   mcdocker --version
   ```

脚本按照仓库的 `Cargo.lock` 编译发行版本，再安装到用户目录。首次构建需要下载依赖。

## 自定义安装与升级

| 需求 | PowerShell 命令 |
| --- | --- |
| 将命令安装到指定目录 | `.\install.ps1 -InstallDir 'D:\Tools\mcdocker'` |
| 自行管理 PATH | `.\install.ps1 -InstallDir 'D:\Tools\mcdocker' -NoPath` |
| 按完整路径检查安装 | `& 'D:\Tools\mcdocker\mcdocker.exe' --version` |

**升级时**，先退出正在用 mcdocker 启动的游戏，再从新版安装包或源码目录重新运行脚本，选择原来的安装目录。脚本会先检查新程序能否输出版本号，再替换已安装的程序。

安装目录用来存放命令。游戏实例可放在另一个工作区，例如：

```powershell
mcdocker --workspace 'E:\minecraftgames' list
```

## 安装遇到问题

- **提示脚本执行被禁止**。确认下载来源后，可用以下命令仅为这次安装启动一个 PowerShell 进程。安装后可直接用完整路径验证，或重新登录 Windows 后使用 `mcdocker`。

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
  & "$env:LOCALAPPDATA\Programs\mcdocker\bin\mcdocker.exe" --version
  ```

- **找不到命令或仍显示旧版本**。用 `Get-Command mcdocker -All` 查看当前终端实际找到的程序。直接执行安装脚本的 PowerShell 会立即使用新路径；其他已有窗口可能保留旧环境，重新登录 Windows 后会读取保存的用户 PATH。
- **源码安装找不到 Cargo 或链接器**。先确认 `cargo --version` 能输出版本号。默认 MSVC 工具链还需要 C++ 桌面开发工具和 Windows SDK。也可以改用预编译包。

[返回快速启动](../README.md#快速启动) · [查看命令参考](commands.md)
