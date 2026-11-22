# superbash 設計（素案）

2026-11-21。設計の素案のみ（code・repo はまだ無い）。推測で書いた所は「推測」と書く。利用者が決めていない事を書く時は「提案（利用者未承認）」と明記する。決まっていない事は 11 章の未決に論点だけ書く（今は無い）。

参考にした物:
- 考え方「任意のシェル文字列を受けず、許した操作だけを道具にする」
- Claude Code の plugin の `marketplace.json`（公開の docs は今回の環境から取れなかった。手元にある公式の marketplace の repo の実物で形を確かめた）

---

## 1. 目的と範囲

### 目的

- shell の操作を、任意の文字列ではなく、toolset に定義した command だけの MCP の道具として出す。どの host でも使える汎用の物にする。
- Rust のネイティブの実装。npx で起動する（npm に OS・CPU ごとの build を載せる）。
- 実行できる command は、toolset の定義の file（`.manifest/toolconfig.json`。marketplace.json に似た位置づけ）で決める。起動の時に `--allow-cmd` で実行できる command を絞れる。
- 同じ toolset の command を、MCP の道具としてだけでなく、名前で打てる普通の command としても入れられる（`--only-install`）。

### やらない事

- Claude Code の組み込みの Bash を塞ぐかどうか（範囲外。触れない）。
- 任意の command 文字列・shell の構文を受ける事。
- host の command（`git` など）を直接実行する事。実行は npx だけ。
- 鍵の受け渡し。非公開の registry と repo には対応しない。
- Windows と、Linux の musl。対象は mac と Linux（glibc）。

---

## 2. 全体の形

### 前提

- superbash に toolset（git の repo か registry の package）を渡すのは、**取って解決するためだけ**。
- npx での実行は常に、**手元で生成した package** に対して行う。registry の toolset を npx で直接呼ばない。
- toolset は command のセットの管理物で、実行の話は持たない。toolset の名前に `@scope` を付けてよい。

### 起動と mode

```
superbash [option...] <toolset>
```

- `<toolset>` は位置引数で 1 つだけ。
- mode:

| mode | 書き方 | 動作 |
|---|---|---|
| MCP（既定） | `superbash <toolset> [--allow-cmd …] [--sandbox] [--host <host>] [--port <port>]` | Streamable HTTP の MCP server として待ち受け（既定 `127.0.0.1:7391`、endpoint は `/mcp`）、道具 `superbash-command` を出す |
| install | `superbash --only-install <toolset> [--allow-cmd …]` | 生成した package を superbash の置き場に置き、`npm link` で入れる（command を名前で打てるように）。MCP は起動しない |

- MCP の mode の待ち受け（利用者の決定: stdio の口は持たず、Streamable HTTP にする）:
  - `--host`（既定 `127.0.0.1`）・`--port`（既定 `7391`。`0` なら空き port）で待ち受ける。endpoint の path は `/mcp`。
  - 起動したら、待ち受けの address を 1 行（stdout）に出す: `superbash: listening on http://<host>:<port>/mcp`。
  - MCP の仕様（Streamable HTTP）が localhost の server に求める DNS rebinding の対策として、Host は loopback だけ、Origin は loopback の物だけを通す（Origin が無い非 browser の client は通る）。仕様に無い認証は足さない。
  - 既定の port 7391 は、Linux・mac の一時 port の範囲（32768 以上）の外で、よく使われる開発用の port（3000・5173・8000・8080・8888・9229 など）とぶつからない番号として選んだ。
- `--allow-cmd`（MCP の mode と install の mode の両方。install の mode では「入れる command」になる）:

| 指定 | 実行できる command |
|---|---|
| 指定無し | toolset の全 command |
| `--allow-cmd 'cmdA,cmdB'` | `cmdA` と `cmdB` だけ。名前は並びの名前（3.4）で、群の名前ならその下の全部、葉の名前ならその葉だけ（例 `--allow-cmd 'github release,gitea repo create'`）。知らない名前は警告だけ。install の mode で絞れるのは tool の bin の単位まで |
| `--allow-cmd ''` | 無し（MCP の mode では実行できる command 無しで起動する。将来の、道具を呼ぶ時の許可の処理に対応させるため。install の mode では入れる command 無し） |

### 流れ（MCP の mode）

```
MCP client（Claude Code の session）
   │ Streamable HTTP（JSON-RPC、POST /mcp）
   ▼
superbash <toolset> [--allow-cmd …]
   1. toolset の取得: <toolset> を npm に渡して取らせる（repo か package かの見分けも npm に任せる）
   2. 定義の読み込み: 取った toolset の .manifest/toolconfig.json を読む
   3. package の生成: 実行できる command の分だけ bin を並べた package.json と、MCP の mode 用の wrapper を組み立て、npm pack で tarball にする
   4. 道具の公開: 道具 superbash-command(name, args) を 1 つ出す
   5. 実行: 呼ばれたら npx で生成した package の wrapper を起動 → wrapper が args（JSON）を spec.in の規則で command の普通の引数に組み立てて起動 → 結果を返す
```

### toolset の取得

- superbash は host の `npm` を shell を通さずに起動し、`npm pack <toolset> --json` で toolset の tarball を取る。repo か registry の package かの見分けは npm に任せる（superbash は見分けの規則を持たない）。
- 取った物は npm の cache に入る。superbash は自前の toolset の cache を持たない。
- 版: registry の package は、位置引数の `<name>@<版>` で書き手が指定できる（MCP の設定の json に書く）。repo は既定の branch を使う。
- 返り値の JSON に、解決した `name`・`version`・`filename` が入る（この Mac の npm 11.19.1 で `--json` の項目を確かめた。3.5）。

### package の生成

- superbash の置き場（生成した package を置く dir）に、次を組み立てる:
  - `package.json`: `name`・`version`・`bin`（実行できる command が使う `tools/<name>` ごとに `"<name>": "tools/<name>/<入口>"`、と MCP の mode 用の wrapper の bin）・`dependencies`（toolset の `package.json` から写す）。
  - `tools/<name>/`: 取った toolset から写す。
  - `.manifest/toolconfig.json`: 写す（wrapper が spec を読むため）。
  - wrapper: MCP の mode 用。command 名と args（JSON）を受け、`toolconfig.json` の `spec.in` の規則で command の普通の引数に組み立て、command を起動する。
  - 振り分けの file（`dispatch/run.mjs`。版 0.3.0 の busybox 化）: `tools.<name>.module` のある tool が 1 つでもある時だけ置く。そのような tool の `bin` はこの file を向く。呼ばれた link の名前（only-install）か第 1 引数（MCP の mode の wrapper が tool の名前を最初に渡す）で tool を選び、module の `run(rawArgs)` が返す数を終了 code にする。module の無い tool は今のとおり入口を直に起動する。決めた方式は docs/toolset-convention.md の 5.4。
- `npm pack` で tarball にし、npx にはこの tarball の path を渡す（tarball を渡すと npm は `_npx` の下に展開した写しを作る。名前@版の指定では、registry に無い package は引けない。3.5）。
- command は普通の CLI のまま。MCP のための作法（JSON の引数）は wrapper だけが持つ。

### 実行

- 呼ばれた道具の `name` と `args` を wrapper に渡す: `npx --yes --package=<生成した tarball の path> -- <wrapper の bin> <name> <args の JSON>`。superbash はこれを shell を通さずに起動する。
- wrapper は並びの名前で葉を引き、`spec.in` の規則で argv の配列を組み（4 章）、command の bin を shell を通さずに起動する。module のある tool は、振り分けの file を `<tool の名前> <argv>` で起動する（子の process を起こす点は同じ。同じ process では実行しない）。
- 上限・background の扱いは 4 章。

### only-install の mode

- 生成した package（wrapper は入れない。利用者の決定: wrapper は MCP の mode だけ）を superbash の置き場に置き、その dir で `npm link` する。npm の global の prefix の `bin/` に command の名前の link ができ、`lib/node_modules/` に生成した dir への link ができる（3.5 の実測）。
- 打ち方は 2 つ（利用者の決定）:
  - `cmdA 引数`（npm の global の bin が PATH にある時）
  - `npx cmdA 引数`
- `npx <名前> cmdA` の形は持たない（振り分け役の bin は作らない）。
- 入れる command の範囲は `--allow-cmd` の規則のとおり（指定無し = 全部、一覧 = その command だけ、`''` = 入れる command 無し）。
- 置き場の生成した dir を消すと link が切れる（3.5 の実測）。

### 将来の狙い（busybox 化・sandbox）

利用者が「今の案」として示した、superbash に一括で手当てさせる理由:

1. **busybox 化**: superbash が command を用意する工程で、command 群を busybox のような 1 つの多機能の実行 file にまとめる。（0.3.0 で、まとめる先を「1 つの実行 file」ではなく「1 つの振り分けの file」に決めて実装した。docs/toolset-convention.md の 5.4。single executable application は作らない。）
2. **sandbox の中での実行**: superbash が、sandbox の中で実行する command 群を組み立てる。（0.4.0 で実装した。次の「sandbox の中での実行」。）

どちらも、toolset の取得から command の用意までを superbash が 1 か所で持つ事でできる。

### sandbox の中での実行（0.4.0。利用者の決定）

- **S1 起動の option**: `--sandbox`（MCP の mode だけ）。既定は無しで、今の振る舞いは変わらない。`--only-install` は対象外で、一緒に指定すると起動の時に誤りで終わる。
- **S2 仕組み**: macOS は `/usr/bin/sandbox-exec` に、生成した profile（SBPL）を `-p` で渡す。Linux は PATH の `bwrap`（bubblewrap）に引数を渡す。どちらも引数の配列で組み、shell を通さない。`--sandbox` を指定して仕組みが無い時、または仕組みがあっても使えない時（user namespace が許されない等）は、起動の時に（待ち受ける前に）理由を出して終了する。
- **S3 絞る範囲**: 書き込みとネットワークだけ。読み取りは絞らない。
  - Linux: `--ro-bind / /`、`--dev /dev`、`--proc /proc`、write の path を `--bind`（在る path だけ）、network false なら `--unshare-net`、`--die-with-parent`。
  - macOS: `(allow default)`、`(deny file-write*)` の後に write の path の `subpath` と `/dev/null` などだけを許す。network false なら `(deny network*)`（unix socket だけ許す）。path は symlink を解決した実体で書く（`/tmp` → `/private/tmp`）。
- **S4 方針（manifest）**: `tools.<名前>.sandbox = { "write": [path...], "network": true|false }`。宛先・アドレスは書かない（知らない key は読み込みで誤り）。
  - path は `~`（home）・`$TMPDIR`・`.`（superbash の作業 dir）で始められる。展開は superbash が行う。それ以外は絶対 path で、相対 path と `..` は誤り。
  - 項が無い tool の既定は network false、write は superbash の置き場（生成した package の dir・background の出力の dir）と一時の dir（`$TMPDIR`・`/tmp`・`/var/tmp`）だけ。
  - 項がある tool の write は、書いた物だけ（置き場と一時の dir は自動では足さない）。network の既定は false。
  - 方針は tool の単位。command の名前から tool を引いて、その tool の方針を使う。
- **S5 実行**: 組んだ「前に置く引数の列」（prefix）を、環境変数 `SUPERBASH_SANDBOX`（JSON の文字列の配列）で wrapper に渡す。wrapper は、tool の process（entry、または振り分けの file）を `prefix + program + argv` で起こす。npx と wrapper 自体は sandbox の外（npm の cache への書き込みを妨げない）。時間切れ・出力・background の扱いは変えない。prefix が読めない時は、sandbox 無しで起こさず止まる。`SUPERBASH_SANDBOX` は tool には渡さない。

---

## 3. toolset と定義の形式

toolset の作法（サブコマンド・道具ごとの dir・citty・busybox の土台。決定の理由と別の案）は docs/toolset-convention.md。この章はその作法のうち superbash が読む形を書く（版 0.2.0 から）。

### 3.1 toolset の構造

```
<toolset>/
  package.json             name・version など。bin は並べない（正本は toolconfig.json）
  .manifest/
    toolconfig.json        定義（正本）
  tools/
    <name>/                各 command の code（1 つの CLI = 1 つの dir）
      <入口>               toolconfig.json の tools.<name>.entry が指す file
```

- toolset は publish して運用する（npm の運用の話）。repo の名前で指す事もできる（その時は既定の branch）。
- `package.json` の例:

```json
{
  "name": "@example/tools",
  "version": "1.2.0",
  "files": [".manifest/", "tools/"]
}
```

- `files` に `.manifest/` を入れれば tarball に入る事は確かめた（3.5）。

### 3.2 marketplace.json との対応

手元の公式の marketplace の repo の形（実物で確認）: repo の中に `.claude-plugin/marketplace.json` があり、plugin の code は同じ repo の `plugins/<name>/` などに置く。最上位は `$schema`・`name`・`description`・`owner`・`plugins`、各 plugin は `name`・`description`・`author`・`category`・`homepage`・`source`（相対 path の `"./plugins/<name>"` か外の取り先）。

| marketplace | superbash の toolset |
|---|---|
| `.claude-plugin/marketplace.json` | `.manifest/toolconfig.json` |
| `plugins/<name>/` | `tools/<name>/` |
| `$schema` / `name` / `description` / `owner` | 同じ |
| `plugins[]` | `commands[]` |
| `plugins[].name` | `commands[].name`（上の要素の名前。並びの名前の先頭。`--allow-cmd` と道具の `name` に使う） |
| `plugins[].description` / `category` | 同じ |
| `plugins[].source` | `commands[].tool`（`tools/<name>` の `<name>`）と `tools.<name>.entry`（入口） |
| （無し） | `commands[].version`・`commands[].spec`（`subcommand`・`usage`・`in`・`out`）・`commands[].subcommands`（サブコマンド。下の要素の配列） |

### 3.3 例（`.manifest/toolconfig.json`）

`spec.in` の書き方（wrapper が引数を組み立てる規則）の具体の形は実装で決める（下は例）。

```json
{
  "$schema": "https://<superbash の schema の置き場>/toolconfig.schema.json",
  "name": "example-tools",
  "description": "project の確認に使う command",
  "owner": { "name": "example" },
  "tools": {
    "lint-cli": { "entry": "main.js" }
  },
  "commands": [
    {
      "name": "lint",
      "description": "TypeScript の lint を走らせる。path を指定すると、その下だけ。",
      "category": "check",
      "tool": "lint-cli",
      "version": "2.4.0",
      "spec": {
        "subcommand": "check",
        "usage": "lint-cli check [--fix] [<target>]",
        "in": {
          "options": {
            "fix": { "flag": "--fix", "type": "boolean", "description": "直せる物を直す" }
          },
          "positionals": [
            { "name": "target", "type": "string", "description": "対象の dir。省略時は cwd", "optional": true }
          ]
        },
        "out": {
          "stdout": { "format": "text", "description": "見つけた問題を 1 行 1 件" },
          "stderr": "進捗と error",
          "exitCodes": { "0": "問題なし", "1": "問題あり", "2": "使い方の誤り" }
        }
      }
    }
  ]
}
```

`superbash-command(name="lint", args={"fix": true, "target": "src"})` で呼ばれた時、wrapper が組み立てて起動する argv:

```
lint-cli check --fix src
```

サブコマンドを持つ例（github → release → create の 3 段。全文は docs/toolset-convention.md の 3.4）:

```json
{
  "tools": { "publish": { "entry": "dist/cli.mjs" } },
  "commands": [
    {
      "name": "github",
      "description": "GitHub の repo と release を作る",
      "tool": "publish",
      "spec": { "subcommand": "github" },
      "subcommands": [
        {
          "name": "release",
          "description": "release の操作",
          "subcommands": [
            {
              "name": "create",
              "description": "release を作る",
              "spec": {
                "usage": "publish github release create [--title <title>] [--draft] <repo> <tag>",
                "in": {
                  "options": {
                    "title": { "flag": "--title", "type": "string" },
                    "draft": { "flag": "--draft", "type": "boolean" }
                  },
                  "positionals": [
                    { "name": "repo", "type": "string" },
                    { "name": "tag", "type": "string" }
                  ]
                }
              }
            }
          ]
        }
      ]
    }
  ]
}
```

`superbash-command(name="github release create", args={"title": "v1.0.0", "draft": true, "repo": "example/app", "tag": "v1.0.0"})` で呼ばれた時の argv:

```
publish github release create --title v1.0.0 --draft -- example/app v1.0.0
```

### 3.4 項目

- `tools.<name>.entry`: `tools/<name>/` からの相対 path の入口（package.json に bin を並べないので、ここが入口の正本）。
- `tools.<name>.module`（省ける。0.3.0 から）: `tools/<name>/` からの相対 path の module。読み込んでも走らず、`main`（citty の command）と `run(rawArgs: string[]): Promise<number>` を export する。あれば `bin` と wrapper は振り分けの file（`dispatch/run.mjs`）を通る。
- 要素: `commands[]` の 1 つ（上の要素）か、その下の `subcommands[]` の 1 つ。同じ形の要素の配列を入れ子に何段でも持てる。子を持つ要素を群、持たない要素を葉と呼ぶ。
- `commands[].name`: 上の要素の名前。サブコマンドの `name` はその要素の名前（argv の語にもなる）。
- `commands[].tool`: `tools` の key（上の要素だけ。下の要素は上の要素の tool）。
- `commands[].version`: その command の版。書き手が指定する（上の要素だけ）。
- `subcommands`: 子の要素の配列（上の要素・下の要素のどちらにも持てる）。
- `spec`（葉は必須、群は省ける）:

| 項目 | 意味 |
|---|---|
| `subcommand` | bin の直後に付ける語（上の要素だけ。下の要素の `spec` には書けない） |
| `usage` | 1 行の使い方 |
| `in` | 入力の定義。wrapper はこの規則で args（JSON）を command の普通の引数に組み立てる（形は 3.3 の提案）。args は呼ぶ葉の `in` だけで読む（群の `in` は子を呼ぶ時には使わない） |
| `out` | 出力の定義（stdout の形式と説明・stderr の説明・終了 code の意味） |

- **並びの名前**: 上の要素から、その要素までの `name` を上から順に空白で区切って並べた物（例 `github release create`）。道具の `name`・`--allow-cmd` の名前に使う。前後の空白は捨て、続いた空白は 1 つの区切りとして読む。上の要素だけなら今の名前と同じ（`subcommands` の無い今の形の toolset は、そのまま動く）。
- **検査**: 兄弟の `name` は重ならない。`name` に空白を含めない。`spec.subcommand` は上の要素だけ。葉は `spec` が必須。呼べるのは葉だけ（群を呼ぶと、子の名前の一覧を誤りで返す）。
- **help**: bin の直後の語 `help` は予約（`<bin> help [<並びの名前>]` が spec の内容を出す。上の要素の `spec.subcommand` と、`spec.subcommand` の無い上の要素の 1 段目の子の名前には使えない）。すべての bin はこの `help` を持つ。superbash は起動の時に `help` を実行して定義と照合しない（定義を正とする）。
- **宛先**: manifest（`description`・`usage` の例を含む）・`tools/<名前>/` の code・toolset の host の設定に、ネットワークの宛先・アドレスを書かない（作法。superbash は見分けて検査しない）。
- **道具ごとの dir**: `tools/<名前>/` の推奨構成（`src/main.ts`・`src/cli.ts`・`src/command/`・build の結果 `dist/cli.mjs` を entry が指す）と、TS で書く時の citty の書き方は docs/toolset-convention.md の 4 章。superbash は `tools/<名前>/` の中だけを生成した package に写す。

### 3.5 npm・npx の実測（この Mac。node 26.8.2・npm 11.19.1。使い捨ての dir・prefix・cache で確かめた）

registry（verdaccio など）はこの Mac に無く、registry から取る形は確かめていない（**未確認**）。GitHub もこの環境から名前解決できなかった。registry に行くかは、registry を届かない `http://127.0.0.1:9/` に向けて確かめた。

**(1) toolset の tarball と生成した package**

| 確かめた事 | 結果 |
|---|---|
| `npm pack --json` の出力 | `name`・`version`・`integrity`・`shasum`・`filename`・`files` が出た |
| `files` に `.manifest/` を入れた時 | tarball に入った |
| tarball の中の実行の権限 | 保たれた |
| 生成した package を `npm pack` し、tarball を `npx --yes --package=<tgz> -- <bin> 引数` | 動いた。2 回目は約 0.4 秒 |
| 生成した dir の path を渡した時 | 写さず元の dir への symlink になった |

**(2) 名前@版での呼び出し（registry に無い生成した package）**

| 確かめた事 | 結果 |
|---|---|
| tarball の path で一度入れた後、`npx --package=<name>@<version> -- <bin>` | 失敗。registry に行く: `GET http://127.0.0.1:9/<name> attempt 1 failed with ECONNREFUSED` |
| 同じく `--offline` | 失敗: `ENOTCACHED … cache mode is 'only-if-cached' but no cached response is available.` |
| 同じく `--prefer-offline` | 失敗（registry に行き `ECONNREFUSED`） |
| `npm cache add <tarball>` の後 | cache の key は `pacote:tarball:file:<path>` で、名前@版では引けず、上と同じ失敗 |

**(3) `npm link`（only-install の形）**

| 確かめた事 | 結果 |
|---|---|
| 生成した dir で `npm link` | 通った。`<prefix>/bin/<bin> -> ../lib/node_modules/<name>/<入口>` と `<prefix>/lib/node_modules/<name> -> <生成した dir>` の symlink |
| `<prefix>/bin` を PATH に足して `<bin>` | 動いた |
| `npx <bin>` | registry に行かずに動いた（PATH に無くても、`--offline` でも） |
| `npx --package=<name> -- <bin>` | 失敗。registry に行く（`ECONNREFUSED`） |
| 生成した dir を消した後 | link は残り行き先が無い。`<bin>` は `No such file or directory`、`npx <bin>` は registry に行って失敗 |

**(4) 振り分け役の bin（`npx <名前> cmdA`。この形は採らなかった。記録として残す）**

生成した package に、各 command の bin に加えて、第 1 引数で command を選ぶ振り分け役の bin を足し、`npm link` した。

| 確かめた事 | 結果 |
|---|---|
| scope 無し（package 名と振り分け役の bin が `example-tools`） | `cmdA`・`npx cmdA`・`npx example-tools cmdA` の 3 つとも registry に行かずに動いた（`--offline` でも） |
| scope 付き（package 名 `@scope/tools`、振り分け役の bin `tools`） | `cmdA2`・`npx cmdA2`・`npx tools cmdA2` は動いた |
| 同じく `npx @scope/tools cmdA2`（`@版` 付き、`--package=@scope/tools -- tools`、link でなく `npm install -g` の後も） | 失敗。registry に行く: `GET http://127.0.0.1:9/@scope%2ftools … ECONNREFUSED`、`--offline` では `ENOTCACHED` |

**(5) git の指定を npx に直接渡した時（前の案で確かめた事）**

| 確かめた事 | 結果 |
|---|---|
| `npx --yes --package=git+file://<repo>#<tag> -- <bin>` | 動いた |
| 引数（`a b`・`--x`・`;ls`・`$(id)`・`` `id` ``・`a'b`・`*`・`x"y`） | 1 引数ずつそのまま届いた |
| git から取る時の `prepare` の script | 走った |
| 無い bin の名前 | `sh: <名前>: command not found`、終了 code 127 |

- 無い bin の時の message から、npx は bin を `sh -c` で起動し、引数を npm が quote していると見られる（推測。上の引数の test では展開は起きなかった）。superbash 自身は shell を通さないが、npx の中で shell を 1 段通る。

---

## 4. 実行の規則

1. **shell を通さない。** superbash は npx を、wrapper は command の bin を、引数の配列で直接起動する（npx の中で shell を 1 段通る事は 3.5 の (5)）。
2. **引数。** wrapper が並びの名前で葉を引き（群・知らない名前・葉の下の名前は使い方の誤り。終了 code 2）、args（JSON）から argv の配列を組む。args は呼ぶ葉の `spec.in` だけで読む。

```
argv = [上の要素の spec.subcommand（あれば）]
     + [上の要素より下の、呼ぶ葉までの name（上から順）]
     + options（定義の順）
     + [`--`]（positional を 1 つでも渡す時だけ）
     + positionals（定義の順）
```

   `--` を入れるのは、`-` で始まる値が option と読まれず、1 引数のまま届くため（command は `--` を受ける事）。子の無い上の要素では、`spec.subcommand` + options + `--` + positionals になる。
3. **作業 dir。** 基点は決めない。path による拒絶もしない。
4. **時間の上限。** 既定は全 command 共通で 10 分。呼ぶ時にその実行の上限を指定できる。
5. **background。** 振る舞いは Claude Code の普通の Bash と同じ: 呼ぶ時に background の実行を指定でき、後から出力を読める。読み方の形は 6 章。

---

## 5. toolset の置き場と信頼

- toolset は npm に渡して取らせる。取った物は npm の cache に入る。
- superbash が持つのは、生成した package の置き場だけ。
- only-install で入れた command は、置き場の生成した dir への link なので、置き場の dir を消すと使えなくなる（3.5 の (3)）。
- 置き場の場所は npm の cache の場所の下（例 `~/.npm/_superbash/`。利用者の決定）。

---

## 6. MCP の道具としての見せ方

### 道具

道具は 1 つ: `superbash-command`。

| 引数 | 意味 |
|---|---|
| `name` | command の名前。サブコマンドは空白で区切った並び（例 `github release create`）。呼べるのは葉だけ |
| `args` | command への引数（JSON）。wrapper が `spec.in` の規則で普通の引数に組み立てる |
| `timeout` | この実行の時間の上限。省略時は 10 分 |
| `run_in_background` | background で実行する |

- 道具の説明に、実行できる command を木の形で並べる（model が `name` と `args` を書けるように）。葉は `name`・`description`・`usage`・`in` を全部、`out` は短く（`stdout.format` と終了 code の意味だけ。`stdout.description` の長文は外す）。群は `name`・`description` と子。下の要素の葉には、呼ぶ時の `name`（並びの名前）を添える。量が増える時は `--allow-cmd` で群の単位に絞る。
- 道具の input schema の `name` の説明に「サブコマンドは空白で区切った並び」を書く。

### 返り値

- Claude Code の普通の Bash と同じく、出力（stdout・stderr）と終了 code を返す。
- background の時は、実行の id と出力を読む先を返し、後から出力を読める。利用者の決定「普通の Bash と同じ」に従い、出力を file に書き、その path を返す。

### 失敗の形

- 実行できない `name`（toolset に無い・`--allow-cmd` に無い・`--allow-cmd ''` で起動した時）は、error として返す。
- 群の `name` は、子の名前の一覧を添えた error として返す（呼べるのは葉だけ）。
- 時間の上限を超えた時は、止めて、その時までの出力と「時間切れ」を返す。

---

## 7. 配布

### superbash 本体

- Rust の 1 本の実行 file。MCP は Rust の MCP SDK（`rmcp` を想定。推測）。
- npm に OS・CPU ごとの build を載せる（esbuild・biome と同じ形）: 本体の package は小さい JS の起動 script と `optionalDependencies`、OS・CPU ごとの package は `os`・`cpu` を書いた `package.json` と実行 file。
- 対象: `darwin-arm64`・`darwin-x64`・`linux-x64`（glibc）・`linux-arm64`（glibc）。
- 起動の例: `npx -y superbash@<版> @example/tools@1.2.0`
- 動かす host には node と npm（npx）が要る。

### toolset

- `.manifest/toolconfig.json`・`tools/`・`package.json` を持つ package として publish して運用する（npm の運用の話）。
- 各 command は普通の CLI で、必須の `help` を持つ。

---

## 8. bantou との繋ぎ

bantou では、普通の MCP の設定として書く。center の上の session は center の共通の設定に、server の worker は各 server の config に書く。鍵は要らない。

例（MCP の設定の 1 件。server は常駐させて、client は url で繋ぐ。利用者の決定: stdio ではなく Streamable HTTP）:

```json
{
  "mcpServers": {
    "superbash": {
      "type": "http",
      "url": "http://127.0.0.1:7391/mcp"
    }
  }
}
```

server は `superbash @example/tools@1.2.0 --allow-cmd lint,test` を常駐させて起動する（mac は launchd の user agent。雛形は `contrib/launchd/`）。

mac の署名ビルドは、後の段で、toolset の command の 1 つとして載せる（ビルド窓口の CLI を `tools/<name>/` に置く）。

---

## 9. test と検証

- toolset の取得: registry の package（`@scope` 付きを含む）と repo の名前の両方を npm に渡して取れる事（私的な registry を test の中で立てる。verdaccio など。推測。git は手元の bare repo）。
- package の生成: 実行できる command の分だけ bin が並ぶ事、wrapper が入る事、tarball を npx に渡して動く事。
- `--allow-cmd`: 指定無しで全 command、`'cmdA,cmdB'` でその 2 つだけ、`''` で無し、になる事（MCP の mode では実行できる command、install の mode では入れる command）。
- wrapper: `spec.in` の規則どおりに argv が組まれる事。shell の特殊文字を含む値が 1 引数のまま届く事（npx の中の shell を含めて）。
- 時間の上限: 既定 10 分と、呼ぶ時の指定が効く事。background の実行と、後から出力を読める事。
- only-install: `npm link` で入り、`cmdA` と `npx cmdA` が動く事。
- MCP: MCP の client で `initialize`・`tools/list`・`tools/call` を通す事。Claude Code から `superbash-command` を呼べる事。
- 配布: 各 OS・CPU（mac・Linux glibc）で npx から起動できる事。
- sandbox: 引数・profile の組み立て（OS に依らない unit test）。bwrap がある Linux では、write の外への書き込みと、network false の時の外（127.0.0.1 の test の server）への接続が止まる事。macOS では sandbox-exec で同じ事。仕組みが無い host では skip。`--sandbox` 無しで今の test がそのまま通る事。
- 静的な検査: source に shell を通す起動（`sh -c` など）が無い事。

---

## 10. 段階

| 段 | 中身 |
|---|---|
| 第 1 段 | この素案の決定の全部（MCP の mode・`--only-install`・`--allow-cmd`・道具 `superbash-command`・wrapper・時間の上限と background・mac と Linux glibc の build） |
| 後の段 | mac の署名ビルドを toolset の command の 1 つとして載せる |
| 0.3.0 | busybox 化（2 章） |
| 0.4.0 | sandbox の中での実行（2 章） |

---

## 11. 未決

今は無い。前の未決 2 件は利用者の決定で確定した:

- only-install の後の打ち方は `cmdA` と `npx cmdA` の 2 つ。`npx <名前> cmdA` は持たない（振り分け役の bin も作らない）。
- only-install で入れる command の範囲は `--allow-cmd` の規則をそのまま使う。
