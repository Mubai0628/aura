# 界面字体

L10.7 所有者选定的字体组合 C（2026-10-19 同意下载）：界面与对话用 Inter 加思源黑体，大标题与思考内容用思源宋体，代码用 JetBrains Mono。只引入下面四个包，版本为确切值；锁文件同时固定版本和 npm 包的 SHA-512 完整性。字体文件随界面打包（安全策略只允许 `font-src 'self'`），运行时按字符范围只加载用到的分片。

| 包                                  | 固定版本 | 来源                                                                                      | npm SHA-512 完整性                                                                              | 许可证                                                |
| ----------------------------------- | -------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| @fontsource-variable/inter          | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/inter/-/inter-5.3.0.tgz                   | sha512-OupL48va4JNofb97w6NYeF9S7W/kHNKM0Er8Dem5nqi4jeOLrVJDoE8tZEpnMJmtkvNbB1EIPPwHcdkF6b1oUA== | OFL-1.1；[完整文本](inter-5.3.0-LICENSE.txt)          |
| @fontsource-variable/noto-sans-sc   | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/noto-sans-sc/-/noto-sans-sc-5.3.0.tgz     | sha512-lNar1dF7Ik/lHNPo/7JWG0TolXY29LtsqYgMvEysooZ5bsO9uH4shJmRrwyJ3PjyTPljhpMJEK0jDuLSU4vJ1w== | OFL-1.1；[完整文本](noto-sans-sc-5.3.0-LICENSE.txt)   |
| @fontsource-variable/noto-serif-sc  | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/noto-serif-sc/-/noto-serif-sc-5.3.0.tgz   | sha512-7LcN2NEf4HDjoSkGWc79WqDDyMK8JvSPdA/gsHjJpZ8l9A9mcK/GQTazp3klmkBwJooy0eqeNvlmaZFXPupHrA== | OFL-1.1；[完整文本](noto-serif-sc-5.3.0-LICENSE.txt)  |
| @fontsource-variable/jetbrains-mono | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/jetbrains-mono/-/jetbrains-mono-5.3.0.tgz | sha512-F32xpS2NsGYoQi2ADSkKTgpJj7ozajsGgDJ8woTnqjmIB+dxDIqImjl4pXZVEExu8UFZ2ndhmX18EBS/hdz3Lw== | OFL-1.1；[完整文本](jetbrains-mono-5.3.0-LICENSE.txt) |

字体项目：Inter（https://github.com/rsms/inter）、Noto Sans SC 与 Noto Serif SC（思源黑体、思源宋体，https://github.com/notofonts/noto-cjk）、JetBrains Mono（https://github.com/JetBrains/JetBrainsMono）；npm 包由 Fontsource（https://fontsource.org）按 Google Fonts 的字符分片发布。SIL Open Font License 1.1 允许随软件打包与再分发，条件是保留版权声明与许可证全文，且不单独出售字体文件；这里使用的文件未经修改。没有引入其他依赖。
