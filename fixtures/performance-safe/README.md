# Public-safe performance fixture

`scripts/generate-performance-fixture.py` で再生成する、架空の QA 専用資料。顧客データ・実写真・実在のブランドは使わない。既存 public-safe と別ディレクトリに置き、製品デモの同梱範囲は変えない。

1,080×1,350、120 visible drawable layers と root group（計121 nodes）、固有合成画像8枚（各1,600×1,000 = 1.6MP）、日本語＋English、Noto Sans JP Regular / Bold / Black 3face、linear/radial gradient、画像clip8つとnative shadow8つを含む。3つの QA 承認済みvariant は見出し位置を明示的に変える。100行CSVも架空の見出しで、300 PNG の入力が固定される。

画像は固定 pixel pattern から生成し、実写真の decode/entropy の代表値とは扱わない。PNG 圧縮 bytes は Python/zlib 版に依存し得る。使用版と全素材・入力の SHA-256 を `fixture-manifest.json` に記録する。フォントは既存の OFL fixture の未改変 copy で、本文を `licenses/` に置く。

検査・計測は `scripts/cargo-verified.sh build --release -p render-cli --locked` の後、他の build/test が停止した状態で `python3 scripts/measure-performance.py` を行う。結果は `out/performance/` に保存する。測るのは headless native CLI の wall/RSS であり、UI操作・frame転送・合算メモリ・通信観測は別の受入試験となる。
