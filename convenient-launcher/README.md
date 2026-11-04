# R1999 启动器

Windows 启动器：启动游戏及三个本地服务，关闭游戏后清理本次启动的服务，记录运行日志。启动时会自动结束占用三个服务端口的进程，请确保这些端口没有其他需要保留的程序。

将程序放在包含 `Server` 和 `Patch` 目录的服务端根目录，复制 `r1999-launcher.ini.example` 为 `r1999-launcher.ini`，并将 `game_dir` 修改为包含 `reverse1999.exe` 的游戏目录。路径可为绝对路径，也可相对于配置文件目录；日志默认写入启动器目录下的 `logs`。

使用 `cargo build --release` 编译。需要嵌入请求管理员权限的清单时，安装 Windows SDK，设置 `R1999_EMBED_ELEVATION=1` 和 `R1999_RC`（指向 SDK 的 rc.exe），再编译。可用 `--root`、`--game-dir`、`--log-dir` 覆盖配置。

每次运行会创建独立日志目录；SDK 日志可能包含登录凭据，分享前请检查并遮盖。
