# Third-party notices

P00 没有仓库内分发的第三方二进制。后续引入 runtime、模型工具或其他可再分发组件时，必须在此记录来源、固定版本、许可证、完整性哈希和再分发条件。

L02 的终端组件见 [xterm 固定版本与许可](xterm.md)。

L10.7 的界面字体见[字体固定版本与许可](fonts.md)。<!-- claude-advisor 2026-10-19 -->L18.10 的像素字体记在同一文件的“像素字体”一节。<!-- claude-advisor 2026-11-04 -->

自动召回随 Core 分发 [Chinese Open Wordnet 1.1](https://bond-lab.github.io/cow/) 的派生同义词表。原始归档 `cow-1.1.tar.xz` 的 SHA-256 为 `93e9357dda50a38ce626904c2229479afa177df74d9c504aebfeba06ff2f241e`。保留[版权、许可与免责声明](chinese-open-wordnet-1.1-LICENSE.txt)，运行 `python scripts/generate-recall-synsets.py <归档路径>` 可重建。
