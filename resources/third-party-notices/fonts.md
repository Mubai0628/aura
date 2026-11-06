# 界面字体

L10.7 所有者选定的字体组合 C（2026-10-19 同意下载）：界面与对话用 Inter 加思源黑体，大标题与思考内容用思源宋体，代码用 JetBrains Mono。只引入下面四个包，版本为确切值；锁文件同时固定版本和 npm 包的 SHA-512 完整性。字体文件随界面打包（安全策略只允许 `font-src 'self'`），运行时按字符范围只加载用到的分片。

| 包                                  | 固定版本 | 来源                                                                                      | npm SHA-512 完整性                                                                              | 许可证                                                |
| ----------------------------------- | -------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| @fontsource-variable/inter          | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/inter/-/inter-5.3.0.tgz                   | sha512-OupL48va4JNofb97w6NYeF9S7W/kHNKM0Er8Dem5nqi4jeOLrVJDoE8tZEpnMJmtkvNbB1EIPPwHcdkF6b1oUA== | OFL-1.1；[完整文本](inter-5.3.0-LICENSE.txt)          |
| @fontsource-variable/noto-sans-sc   | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/noto-sans-sc/-/noto-sans-sc-5.3.0.tgz     | sha512-lNar1dF7Ik/lHNPo/7JWG0TolXY29LtsqYgMvEysooZ5bsO9uH4shJmRrwyJ3PjyTPljhpMJEK0jDuLSU4vJ1w== | OFL-1.1；[完整文本](noto-sans-sc-5.3.0-LICENSE.txt)   |
| @fontsource-variable/noto-serif-sc  | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/noto-serif-sc/-/noto-serif-sc-5.3.0.tgz   | sha512-7LcN2NEf4HDjoSkGWc79WqDDyMK8JvSPdA/gsHjJpZ8l9A9mcK/GQTazp3klmkBwJooy0eqeNvlmaZFXPupHrA== | OFL-1.1；[完整文本](noto-serif-sc-5.3.0-LICENSE.txt)  |
| @fontsource-variable/jetbrains-mono | 5.3.0    | https://registry.npmjs.org/@fontsource-variable/jetbrains-mono/-/jetbrains-mono-5.3.0.tgz | sha512-F32xpS2NsGYoQi2ADSkKTgpJj7ozajsGgDJ8woTnqjmIB+dxDIqImjl4pXZVEExu8UFZ2ndhmX18EBS/hdz3Lw== | OFL-1.1；[完整文本](jetbrains-mono-5.3.0-LICENSE.txt) |

字体项目：Inter（https://github.com/rsms/inter）、Noto Sans SC 与 Noto Serif SC（思源黑体、思源宋体，https://github.com/notofonts/noto-cjk）、JetBrains Mono（https://github.com/JetBrains/JetBrainsMono）；npm 包由 Fontsource（https://fontsource.org）按 Google Fonts 的字符分片发布。SIL Open Font License 1.1 允许随软件打包与再分发，条件是保留版权声明与许可证全文，且不单独出售字体文件；这里使用的文件未经修改。没有引入其他依赖。

## 像素字体

L18.10 灰白像素主题（2026-11-04 所有者同意下载）：标题、标签、按钮、菜单、标记、按键和数字用 Fusion Pixel Font 12px 比例宽度简体中文版，只在 12px 或 24px 下使用；正文、对话、输入框和代码仍用上表字体。字体文件取自 GitHub 发布页，未经修改，随界面打包（`apps/desktop/renderer/src/fonts/`）。

| 文件                                             | 固定版本   | 来源                                                                                                                                    | SHA-256                                                                                                                                            | 许可证                                                        |
| ------------------------------------------------ | ---------- | --------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| fusion-pixel-12px-proportional-zh_hans.otf.woff2 | 2026.10.25 | https://github.com/TakWolf/fusion-pixel-font/releases/download/2026.10.25/fusion-pixel-font-12px-proportional-otf.woff2-v2026.10.25.zip | 压缩包 0501b4f770887eef5fc5ac94401b3b7bb38253ffed3a549e2cb0be89a0a03fe3；字体文件 b67eb7aa592622e5bc1152b800d32404bb887a1e3a2e669b0c2a873d388590d6 | OFL-1.1；[完整文本](fusion-pixel-font-2026.10.25-LICENSE.txt) |

字体项目：Fusion Pixel Font（https://github.com/TakWolf/fusion-pixel-font）。它合并了下列字体的字形，压缩包附带各自的许可证，原样保留：Ark Pixel Font（OFL-1.1，[完整文本](fusion-pixel-font-2026.10.25-ark-pixel-LICENSE.txt)）、Cubic 11（OFL-1.1，[完整文本](fusion-pixel-font-2026.10.25-cubic-11-LICENSE.txt)）、Galmuri（OFL-1.1，[完整文本](fusion-pixel-font-2026.10.25-galmuri-LICENSE.txt)）。
