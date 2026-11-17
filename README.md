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
asis <owner/repo>...
asis <owner/repo> --mine
```

## 状態

作り始めたところ。今あるのは、状態と番を決める規則と、結果を文字で出すコマンド。TUI と GUI はこれから。

## 開発

```
cargo test
cargo clippy --all-targets
```

## ライセンス

MIT
