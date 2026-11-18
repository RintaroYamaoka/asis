# asis

development, as is — 開発を、あるがままに見える化する。

起票を強いる道具は、使われなくなる。asis は何も入力を求めない。GitHub で起きた事実(PR・レビュー・テスト・取り込み・デプロイ)を読み、今どこまで進んでいて、次は誰の番かを出す。

- 状態は誰も動かさない。事実だけで動く
- issue が無い PR も、そのまま見える
- コードも PR も預からない。あなたの PC の上で、あなたの権限で GitHub を読む
- repo には何も置かない。ログインしなくても、1 人ですぐ使える

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

### 窓の画面

```
cargo install --path crates/asis-gui
asis-gui
```

### Windows

`dist/asis-windows.zip` を開いて、中の `install.ps1` を動かす(管理者の権限は要らない)。スタートメニューに「asis」が入る。

```
powershell -ExecutionPolicy Bypass -File install.ps1
```

zip は、Linux か WSL の上で `scripts/build-windows.sh` で作る。

### WSL(仕事は WSL の中、窓は Windows)

WSL の上で `scripts/install-windows.sh` を打つと、Windows 版を作って、その PC の Windows に入れる。
入れたあとは、WSL で `asis-gui` と打っても、スタートメニューの「asis」からでも、Windows 版の窓が開く。
gh のログイン・設定・手元の作業は、WSL の中のものをそのまま使う(窓だけが Windows で動く)。

Windows 版を入れていなければ、WSL の中で窓が出る(`ASIS_BACKEND=wayland` でも)。

## 部屋

repo・メンバー・役割を人と共有するときは、部屋を使う。部屋は DB(Supabase)に置く。DB に入るのは部屋の定義だけで、issue・PR・コードは入らない。

```
asis login                                  # GitHub でログインする(本人確認だけ。repo を読む許可は求めない)
asis room new team                          # 部屋を作る。作った人が管理者
asis room add team owner/api                # repo を足す
asis room invite team alice --review        # GitHub のアカウント名で招く。役割: --review --merge --deploy --admin
asis room ignore team Preview               # 状態を決めるときに数えないチェック
asis room                                   # 入っている部屋を出す
```

- 部屋に入っても、GitHub で見られない repo は見えない。見る権限を決めるのは GitHub だけ
- 部屋のメンバーが、その部屋で出す人になる
- 部屋を作った人は、確認・取り込み・本番の役を全部 持って始まる。招いた人は、指定しなければ役なし
- 確認する役の人が居なければ、確認は作者以外のメンバー。取り込む役・本番に出す役の人が居なければ、作者

既定のつなぎ先は無い。部屋を使うには、自分の Supabase を用意して、手元の設定(`~/.config/asis/config.toml`)に場所を書く。

```toml
[db]
url = "https://xxxx.supabase.co"
key = "公開用の鍵(publishable / anon)"
```

用意のしかた:[supabase/migrations](supabase/migrations) の SQL を順に流す。Authentication で GitHub のログインを有効にし(GitHub に OAuth App を登録する)、Redirect URLs に `http://127.0.0.1:53682/callback` を足す。部屋を作ってよい人の GitHub の番号を、表 `room_creators` に入れる。

## AI に渡す決まり

PR がどの issue の仕事かは、2 つの書き方から読む。ブランチ名を issue の番号で始める(`12-short-name` か `feature/12-short-name`)。PR の本文に `Closes #12` か `Refs #12` を書く。

この 2 つを AI が守るための手順書が [skill/asis/SKILL.md](skill/asis/SKILL.md) にある。Claude Code なら、このフォルダを `~/.claude/skills/asis` に置く。以後、「#12 を始めて」「今の話を issue にして」で効く。

## 手元の作業

この PC の上の worktree とターミナルのペインを読み、どのタスク・どの PR を触っているかを一覧に重ねる(▶ AI が動いている / ◆ ペインあり / ◇ worktree あり)。PR になる前の作業も見える。

clone の置き場は、ペインが開いている場所と今の場所から見つける。ほかの場所にあるときは、手元の設定(`~/.config/asis/config.toml`)に探すフォルダを書く。

```toml
roots = ["~/src"]   # 直下にある、見ている repo の clone と、その worktree を読む
```

## 状態

作り始めたところ。今あるのは、状態と番を決める規則、部屋、ターミナルの画面(TUI)。GUI はこれから。

## 開発

```
cargo test
cargo clippy --all-targets
```

## ライセンス

MIT
