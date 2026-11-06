# 主题

插件可以带一个主题：插件根目录的 `theme.toml`，只改 Aura 窗口的颜色、字体和圆角。用户在 设置 → 环境 → 外观 里选中它才生效；插件没信任、被停用、卸载了或主题有问题时，窗口用 Aura 自己的主题。

一个插件可以只带一个主题，别的什么都没有。Claude Code 格式的插件不读 `theme.toml`。

## 格式

```toml
title = "夜间"     # 必填：在设置里显示的名字，一行，最多 40 个字符
scheme = "dark"   # 可选：light（默认）或 dark，决定滚动条、下拉框这类系统控件的明暗

[colors]          # 每个值是 #rgb、#rrggbb 或 #rrggbbaa
canvas = "#1e1f1c"
ink = "#e6e6e6"

[fonts]           # 每项是 1 到 8 个字体名，前面的优先
ui = ["Inter", "Microsoft YaHei UI"]
mono = ["JetBrains Mono", "Consolas"]

[radius]          # 0 到 24 的整数，单位像素
md = 6
```

`[colors]`、`[fonts]`、`[radius]` 至少写一样；没写的仍是 Aura 自己的。不认识的键和不合格的值都算问题。

## 颜色

| 键                                            | 用途                                                     |
| --------------------------------------------- | -------------------------------------------------------- |
| `canvas`                                      | 窗口的底色                                               |
| `surface`                                     | 卡片、输入框、弹出层的底色                               |
| `surface_muted`                               | 次一级的底色                                             |
| `surface_selected`                            | 选中项的底色                                             |
| `ink`                                         | 正文                                                     |
| `muted`                                       | 次要的文字                                               |
| `quiet`                                       | 更淡的文字                                               |
| `line`、`line_strong`                         | 分隔线和边框；后一个更明显                               |
| `accent`、`accent_strong`、`accent_soft`      | 强调色（按钮、链接、选中）、深一档的强调色、强调色的浅底 |
| `success`、`warning`、`danger`                | 成功、提醒、出错                                         |
| `success_soft`、`warning_soft`、`danger_soft` | 它们的浅底                                               |
| `focus`                                       | 键盘焦点的框                                             |

深色主题要把 `surface` 一类、`line` 一类和 `_soft` 一类一起改，不然会是深色窗口里的浅色卡片。

## 字体

`ui` 是界面和对话，`mono` 是代码、路径和命令，`serif` 是大标题和思考过程。字体名只能有文字、数字、空格和 `-`、`_`、`.`。用户电脑上没有的字体会被跳过，列表最后总会补上同类的系统字体。

## 圆角

`sm`、`md`、`lg`、`xl` 从小到大，用在小按钮、输入框、卡片和对话框上。

## 检查和修改

- `plugins.validate` 把主题的每个问题列成一条 `主题：…`。主题有任何问题就不能用，插件的其他部分照常。
- 开发中的插件改 `theme.toml` 不用重新信任；用户切回 Aura 窗口时就是新的样子。
- 不是开发中的插件，`theme.toml` 和别的文件一样，改了要重新信任。

## 分享插件

插件页的“导出…”把插件存成一个 ZIP 文件，别人在插件页用“导入 ZIP…”装上。ZIP 里正好是插件页读到的那些文件；超过 64 MB 的导不出来。
