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
asis add <owner/repo>         # 見る repo に足す。以後は asis だけで開く
asis --plain                  # 画面を開かず、文字で出す
```

## プロジェクトの定義

まとめる repo・役割・数えないチェックは、repo の中の `.asis.toml` に置く。その repo を見られる人は、同じ定義を読む。ひな形は `asis init` で出る。置かなければ、その repo 1 つを既定の決まりで見る。

```toml
name = "shop"
repos = ["owner/api"]            # この repo と一緒に見る repo
ignore_checks = ["Preview"]      # 状態を決めるときに数えないチェック

[roles]                          # GitHub のアカウント名か "author"(PR を出した本人)
review = ["alice"]               # 確認する人
merge = ["alice"]                # 承認のあと取り込む人(省くと作者)
deploy = ["alice"]               # 本番に出す人(省くと作者)
```

メンバーを招待する機能は無い。GitHub で repo の権限を渡し、相手が `asis add owner/repo` と打てば、同じ画面が出る。

## 状態

作り始めたところ。今あるのは、状態と番を決める規則と、ターミナルの画面(TUI)。手元の作業の表示と GUI はこれから。

## 開発

```
cargo test
cargo clippy --all-targets
```

## ライセンス

MIT
