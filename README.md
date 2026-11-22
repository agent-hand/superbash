# superbash

toolset に定義した command だけを、MCP の道具 `superbash-command` として出す。設計は [docs/superbash-design.md](docs/superbash-design.md)。

## 使い方

```
superbash [--host <host>] [--port <port>] [--allow-cmd <cmds>] <toolset>
superbash --only-install [--allow-cmd <cmds>] <toolset>
```

MCP の mode（既定）は、Streamable HTTP の MCP server として待ち受ける（利用者の決定: stdio の口は持たない）。

- toolset には local の path も渡せる。相対の path は起動した dir から解く。
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

## busybox 化（0.3.0 から）

tool は、実行の入口（`entry`）に加えて、読み込んでも走らない module を出せる。manifest の `tools.<名前>.module`（`tools/<名前>/` からの相対 path。例 `dist/main.mjs`）に書く。module は `main`（citty の command）と `run(rawArgs: string[]): Promise<number>`（使い方の誤りは 2、その他の誤りは 1 の終了 code）を export する。

- superbash は生成した package に振り分けの file（`dispatch/run.mjs`）を置き、module のある tool の bin をそこへ向ける。link の名前（`--only-install`）か、第 1 引数（MCP の mode の wrapper が tool の名前を最初に渡す）で tool を選び、`run` の返す数を終了 code にする。
- MCP の mode は今のとおり子の process を起こす（時間切れ・出力の扱いは変わらない）。
- `module` の無い tool は、今のまま入口を直に起動する（互換）。
- 決めた方式は [docs/toolset-convention.md](docs/toolset-convention.md) の 5.4。

## sandbox（0.4.0 から）

`superbash --sandbox <toolset>`（MCP の mode だけ）で、tool の process を sandbox の中で起こす。絞るのは書き込みとネットワークだけで、読み取りは絞らない。既定は無し（今のとおり）。

- 仕組み: macOS は `/usr/bin/sandbox-exec`（superbash が profile を生成）、Linux は PATH の `bwrap`（bubblewrap。例 `apt install bubblewrap`）。仕組みが無い・使えない時は、起動の時に理由を出して終了する。
- 方針は manifest の `tools.<名前>.sandbox`:

```json
"tools": {
  "build-cli": {
    "entry": "main.js",
    "sandbox": { "write": ["~/.cache/build", "$TMPDIR", "./out"], "network": false }
  }
}
```

  - `write`: 書き込みを許す path。`~`・`$TMPDIR`・`.`（superbash の作業 dir）で始められ、展開は superbash が行う。それ以外は絶対 path。
  - `network`: 外への通信を許すか（既定 false）。宛先・アドレスは書けない。
  - 項が無い tool: network false、write は superbash の置き場（生成した package・background の出力）と一時の dir だけ。項がある tool の write は書いた物だけ。
  - Linux は PATH の絶対 path の要素だけから `bwrap` を探す（空・相対の要素は捨てる）。`HOME`・`TMPDIR` が空・相対 path なら、起動の時に理由を出して終了する（絶対 path にする）。bwrap には `--new-session` を付け、tool を端末から切り離す。
  - 在る write の path だけが bind される。`write` に在るのに存在しない path があれば、起動の時に stderr へ警告を 1 行ずつ出す（止めない）。
- 注意（絞る範囲の限り）:
  - unix socket（`docker.sock` など）は、`network: false` でも絞らない。絞るのは書き込みとネットワーク（通信）で、socket の file に繋ぐ事は止めない。
  - `write` に `~` を入れた tool は、home の外にしか書き込みの制限が効かない（home の下は全部書ける）。
  - project（作業 dir）が home の外にある時は、`write` にその path（例 `.`）を足す必要がある。
- `--only-install` とは一緒に使えない。
- 方式の詳細は [docs/superbash-design.md](docs/superbash-design.md) の 2 章。

## test

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

test は空き port で起動し、起動の 1 行から address を読む。
