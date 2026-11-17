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
asis add <owner/repo> --only alice,bob   # その repo では、その人たちの分だけ出す
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

## AI に渡す決まり

PR がどの issue の仕事かは、2 つの書き方から読む。ブランチ名を issue の番号で始める(`12-short-name` か `feature/12-short-name`)。PR の本文に `Closes #12` か `Refs #12` を書く。

この 2 つを AI が守るための手順書が [skill/asis/SKILL.md](skill/asis/SKILL.md) にある。Claude Code なら、このフォルダを `~/.claude/skills/asis` に置く。以後、「#12 を始めて」「今の話を issue にして」で効く。

## 手元の作業

この PC の上の worktree とターミナルのペインを読み、どのタスク・どの PR を触っているかを一覧に重ねる(▶ AI が動いている / ◆ ペインあり / ◇ worktree あり)。PR になる前の作業も見える。

clone の置き場は、ペインが開いている場所と今の場所から見つける。ほかの場所にあるときは、手元の設定(`~/.config/asis/config.toml`)に探すフォルダを書く。

```toml
roots = ["~/src"]   # 直下にある、見ている repo の clone と、その worktree を読む
```

## メンバー

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
