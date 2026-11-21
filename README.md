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
- `--allow-cmd` と `--only-install` は従来どおり（設計 2 章）。

client の設定（Claude Code の例）:

```json
{ "mcpServers": { "superbash": { "type": "http", "url": "http://127.0.0.1:7391/mcp" } } }
```

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

## test

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

test は空き port で起動し、起動の 1 行から address を読む。
