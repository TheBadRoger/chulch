# chulch

[简体中文](../README.md)

chulch is a third-party launcher and instance manager for **Minecraft: Java Edition on 64-bit Windows (x64)**. Inspired by Docker's layered storage, it saves disk space by sharing identical game files across instances. Each instance keeps its own mods, settings, and worlds.

## Quick start

**1. Install chulch.** Open PowerShell and run these commands. The second command prints the installed version.

```powershell
& ([scriptblock]::Create((irm 'https://raw.githubusercontent.com/TheBadRoger/chulch/main/install.ps1')))
chulch --version
```

**2. Install and launch a modpack.** Replace the example path with your Modrinth `.mrpack` or CurseForge export ZIP. Here, `survival` is the name you give the instance.

```powershell
chulch install '.\my-pack.mrpack' --id survival
chulch launch survival
```

[Installation and updates (Chinese)](install.md) · [Command reference (Chinese)](commands.md)

chulch is an independent, unofficial Minecraft project.
