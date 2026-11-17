# asis

development, as is — 開発を、あるがままに見える化する。

起票を強いる道具は、使われなくなる。asis は何も入力を求めない。GitHub で起きた事実(PR・レビュー・テスト・取り込み・デプロイ)を読み、今どこまで進んでいて、次は誰の番かを出す。

- 状態は誰も動かさない。事実だけで動く
- issue が無い PR も、そのまま見える
- サーバーも DB も持たない。あなたの PC の上で、あなたの権限で GitHub を読む

設計は [docs/design.md](docs/design.md)。

## 使う

GitHub の公式コマンド [`gh`](https://cli.github.com/) でログインしてあること。

```
cargo install --path crates/asis
asis <owner/repo>...          # その repo を開く
asis                          # 設定ファイルのプロジェクトを開く
asis <owner/repo> --plain     # 画面を開かず、文字で出す
```

設定ファイルは `~/.config/asis/config.toml`(Windows は `%APPDATA%\asis\config.toml`)。

```toml
[[project]]
name = "shop"
repos = ["owner/front", "owner/api"]
merger = "reviewer"          # 承認のあと取り込む人。author か reviewer(既定 author)
deployer = "reviewer"        # 取り込みのあと本番に出す人(既定 author)
ignore_checks = ["Preview"]  # 状態を決めるときに数えないチェックの名前
```

## 状態

作り始めたところ。今あるのは、状態と番を決める規則と、ターミナルの画面(TUI)。手元の作業の表示と GUI はこれから。

## 開発

```
cargo test
cargo clippy --all-targets
```

## ライセンス

MIT
