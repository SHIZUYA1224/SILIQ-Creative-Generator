# ADR 0003 — Phase 0で追加したissue code

状態：Phase 0で採用／2026-10-10

docs/05 §9 の主要コードに加え、Phase 0の実装で区別が必要になったコードを追加した。いずれも問題を黙って通さないためのもので、既存コードへの格下げはしていない。

| code | 重大度 | 意味 |
|---|---|---|
| `E_SCHEMA` | error | JSON構文、未知のプロパティ、型・範囲違反、未対応のschemaVersion |
| `E_DOCUMENT_INVALID` | error | 参照切れ、ID重複、木構造の循環・孤立、slotとnodeの型不一致、fitサイズ規則違反 |
| `E_ASSET_CHECKSUM` | error | 素材・フォントのbytesが記録されたSHA-256と一致しない |
| `E_VARIANT_NOT_APPROVED` | error | variantは存在するが未承認（draft） |
| `E_LOGO_MIN_WIDTH` | error | 描画されるロゴ幅が `minWidth` 未満 |
| `E_RENDER_FAILED` | error | Skiaがsurface／shader／encodeでnullを返した（エンジン側の不具合） |
| `W_IMAGE_UPSCALED` | warning | 画像が元画素より拡大されて描画される |

既存コードの使い分け：
- フォント未選択、ファイル不在、faceの読み込み失敗は `E_FONT_MISSING`。
- 素材未選択、ファイル不在は `E_ASSET_MISSING`。
- 素材の内容と宣言型の不一致、壊れた画像、アニメーション、EXIF回転、WebP（このbuildには無い）は `E_ASSET_TYPE`。
- symlink、資源ディレクトリー外、出力名の不正は `E_PATH_DENIED`。
- 出力先に同名ファイルがある場合は `E_OUTPUT_COLLISION`。
