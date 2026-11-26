# 安装与升级

在 **Windows x64** 上打开普通 PowerShell，运行以下命令即可为当前用户安装最新版 chulch。

```powershell
& ([scriptblock]::Create((irm 'https://raw.githubusercontent.com/TheBadRoger/chulch/main/install.ps1')))
chulch --version
```

- **升级**：退出正在运行的 chulch，再运行同一条安装命令。已有实例和存档可以继续使用。
- **指定安装位置**：在安装命令末尾加上 `-InstallDir 'D:\Tools\chulch'`。加上 `-NoPath` 可自行管理命令路径。
- **确认安装位置**：运行 `(Get-Command chulch).Source`。默认位置是 `%LOCALAPPDATA%\Programs\chulch\bin`。
- **手动安装**：从最新版发布页下载 `chulch-windows-x64.zip`，解压后在该目录运行 `.\chulch.exe --version`。[下载 Windows 版](https://github.com/TheBadRoger/chulch/releases/latest)

从旧版 mcdocker 升级后，用 `chulch list` 查看原有实例。若曾在 PowerShell 配置中启用补全，将那一行里的 `mcdocker` 改为 `chulch`。

[返回快速启动](../README.md#快速启动) · [命令参考](commands.md)
