# superbash

toolset に定義した command だけを、MCP の道具 `superbash-command` として出す。設計は [docs/superbash-design.md](docs/superbash-design.md)。

## 使い方

```
superbash [--host <host>] [--port <port>] [--allow-cmd <cmds>] <toolset>
superbash --only-install [--allow-cmd <cmds>] <toolset>
```

MCP の mode（既定）は、Streamable HTTP の MCP server として待ち受ける（利用者の決定: stdio の口は持たない）。

- 既定の待ち受けは `127.0.0.1:7391`、endpoint は `/mcp`。`--host`・`--port` で変える。`--port 0` は空き port。
- 起動すると、待ち受けの address を 1 行（stdout）に出す: `superbash: listening on http://127.0.0.1:7391/mcp`
- `--allow-cmd` と `--only-install`（設計 2 章）。`--allow-cmd` の名前は並びの名前（空白で区切る）で書ける: 群の名前（`'github release'`）ならその下の全部、葉の名前（`'github release create'`）ならその葉だけ。空白を含むので shell では quote する。知らない名前は警告だけ。`--only-install` で絞れるのは tool の bin の単位まで。

client の設定（Claude Code の例）:

```json
{ "mcpServers": { "superbash": { "type": "http", "url": "http://127.0.0.1:7391/mcp", "timeout": 660000 } } }
```

#### 長い command と呼び出しの上限

- Claude Code は http の MCP の 1 回の呼び出しを、既定で 60 秒で打ち切る（進み具合の通知では延びない）。設定の `timeout`（ms）で延ばせる。上の例の `"timeout": 660000`（11 分）は、superbash の既定の command の時間の上限（10 分）より少し長い。
- 数十秒以上かかりうる command（build・publish など）は、`run_in_background: true` で呼ぶ。
- 打ち切られても、command は最後まで走る（呼び出しが切れても、command の process は止まらない）。「時間切れ」と出ても、結果は出ている事がある。公開・作成のように呼び直すと二重になる command は、呼び直す前に結果（公開されたか・作られたか）を確かめる。

### command の作業 dir

`superbash-command` に作業 dir の引数は無い。http の server では、command の作業 dir は **server を起動した dir** になる。「省略時は今の dir」の command は、その dir で動く。dir が要る時は、command の引数で path を渡す（設計 4 章 3）。

### 既定の port（7391）の理由

固定の 1 つにして、client の設定に書いておけるようにした。他の道具とぶつかりにくい番号として、次を満たす物を選んだ。

- Linux・mac の一時 port の範囲（32768 以上）の外。client が外へ繋ぐ時の port と取り合わない。
- よく使われる開発用の port（3000・5173・8000・8080・8888・9229 など）ではない。

ぶつかる時は `--port` で変える。

### 安全の扱い

- 既定は `127.0.0.1` だけで待ち受ける。
- MCP の仕様が localhost の server に求める DNS rebinding の対策として、Host は loopback だけ、Origin は loopback の物だけを通す（Origin を付けない非 browser の client は通る）。違えば 403。`--host` に loopback 以外を渡した時は、その host も許可に足す。
- 認証は無い（仕様に無い）。待ち受けられる範囲 = 使える範囲なので、`--host` を広げる時は注意する。

## 常駐（macOS の launchd）

user agent として常駐させる雛形が [contrib/launchd/io.example.superbash.plist](contrib/launchd/io.example.superbash.plist)。root は使わない。

1. 雛形を写し、placeholder を書き換える: `npx`・`node` の path、port、toolset、log の置き場（`/path/to/...`、`@scope/toolset@1.0.0`、`7391`）。Label も自分の物にしてよい。
2. `~/Library/LaunchAgents/<Label>.plist` に置く。
3. 読み込む: `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/<Label>.plist`
4. 外す: `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/<Label>.plist`

作業 dir（plist の `WorkingDirectory`）: launchd が server を起動する dir になり、そのまま command の作業 dir になる。雛形では `/path/to/workdir` を placeholder にしてある。command を動かしたい dir（例: 作業の repo を置いた dir）に書き換える。省略すると launchd の既定（`/`）になり、「省略時は今の dir」の command は `/` で動くので、既定の作業 dir を決めたいなら必ず指定する。

（古い書き方では `launchctl load <plist>` と `launchctl unload <plist>`。）起動したかは log の address の 1 行で分かる。

## サブコマンド（0.2.0 から）

command は `subcommands`（同じ形の要素の配列。入れ子は何段でも）を持てる。作法は [docs/toolset-convention.md](docs/toolset-convention.md)、読む形は設計 3 章。

- 呼ぶ時の `name` は、空白で区切った並び（例 `github release create`）。呼べるのは葉だけで、群を呼ぶと子の名前の一覧が誤りで返る。`args` は呼ぶ葉の `spec.in` だけで読む。
- wrapper の argv は、bin の後に `spec.subcommand`（上の要素にあれば）と、上の要素より下の名前の並び、options、`--`、positionals の順。`-` で始まる positional も 1 引数のまま届く（command は `--` を受ける事）。
- 道具の説明は木の形。葉は `name`・`description`・`usage`・`in` を全部、`out` は `stdout.format` と終了 code の意味だけ。
- `subcommands` の無い今の形の toolset は、そのまま動く（ただし positional の前に `--` が入る）。
- 新しい形の toolset は superbash 0.2.0 以上で使う（0.1.2 は `spec` の無い群を「形が不正」で止め、`spec` のある群の `subcommands` は黙って捨てる）。

## test

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

test は空き port で起動し、起動の 1 行から address を読む。
