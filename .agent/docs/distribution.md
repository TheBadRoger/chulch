# 整合包分发与下载源

## 首版导入

| 格式 | 识别依据 | 安装规则 |
| --- | --- | --- |
| Modrinth `.mrpack` | ZIP 根目录的 `modrinth.index.json` | 读取 Minecraft 与加载器版本、文件路径、下载地址、哈希和客户端环境要求。应用 `overrides`，再应用 `client-overrides`。跳过服务端专用条目与 `server-overrides` |
| CurseForge 导出 ZIP | ZIP 根目录的 `manifest.json` | 使用 `projectID` 和 `fileID` 精确解析文件，处理必选与可选项，导入清单指定的 overrides。为 API 无法提供下载的文件展示所需版本及手动补齐入口 |

整合包清单归一为同一安装计划。包来源与下载传输地址分别记录。手动补齐只接受目标条目，使用来源可提供的哈希和大小验证。缺少可信哈希时保留来源、验证状态及本地 SHA-256。整个计划完成后再使实例可启动。

## 下载源路由

| 资源 | 默认上游 | 可配置镜像 |
| --- | --- | --- |
| Minecraft 元数据、客户端、资源与部分公共库 | Mojang 对应源 | BMCLAPI 与兼容的自定义镜像按资源类型启用 |
| Fabric、Forge、NeoForge 元数据、安装器与 Maven 依赖 | 各项目声明的官方源 | BMCLAPI 的适用端点及自定义 Maven 镜像，逐项验证覆盖 |
| 整合包模组 | Modrinth 清单声明的公开地址或 CurseForge API 解析结果 | 仅使用明确配置且适用于该资源的合规镜像。下载受限时提示手动补齐 |

CLI 应支持列出各源能力、调整优先级和诊断实际命中。按资源类型决定候选顺序。镜像缺件、超时或校验失败时尝试下一个候选，保留官方回退。缓存通过可信上游提供的完整性信息校验，镜像自身的数据不能独立证明文件等同于上游。认证请求使用认证服务自己的端点，镜像只能接收对应的公开下载请求。

验收至少覆盖官方源正常、BMCLAPI 命中、镜像缺件回退、内容校验失败、用户自定义镜像及 CurseForge 受限文件。镜像覆盖范围随版本和文件类型变化，发布时记录已验证的路由。

## 参考

- [Modrinth 整合包格式](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack)
- [CurseForge 导出格式](https://support.curseforge.com/support/solutions/articles/9000197908)
- [CurseForge REST API](https://docs.curseforge.com/rest-api/)
- [BMCLAPI 国内分发网络](https://github.com/bangbang93/openbmclapi)
