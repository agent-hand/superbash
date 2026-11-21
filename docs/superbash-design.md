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
| MCP（既定） | `superbash <toolset> [--allow-cmd …] [--host <host>] [--port <port>]` | Streamable HTTP の MCP server として待ち受け（既定 `127.0.0.1:7391`、endpoint は `/mcp`）、道具 `superbash-command` を出す |
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
| `--allow-cmd 'cmdA,cmdB'` | `cmdA` と `cmdB` だけ |
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
- `npm pack` で tarball にし、npx にはこの tarball の path を渡す（tarball を渡すと npm は `_npx` の下に展開した写しを作る。名前@版の指定では、registry に無い package は引けない。3.5）。
- command は普通の CLI のまま。MCP のための作法（JSON の引数）は wrapper だけが持つ。

### 実行

- 呼ばれた道具の `name` と `args` を wrapper に渡す: `npx --yes --package=<生成した tarball の path> -- <wrapper の bin> <name> <args の JSON>`。superbash はこれを shell を通さずに起動する。
- wrapper は `spec.in` の規則で argv の配列を組み、command の bin を shell を通さずに起動する。
- 上限・background の扱いは 4 章。

### only-install の mode

- 生成した package（wrapper は入れない。利用者の決定: wrapper は MCP の mode だけ）を superbash の置き場に置き、その dir で `npm link` する。npm の global の prefix の `bin/` に command の名前の link ができ、`lib/node_modules/` に生成した dir への link ができる（3.5 の実測）。
- 打ち方は 2 つ（利用者の決定）:
  - `cmdA 引数`（npm の global の bin が PATH にある時）
  - `npx cmdA 引数`
- `npx <名前> cmdA` の形は持たない（振り分け役の bin は作らない）。
- 入れる command の範囲は `--allow-cmd` の規則のとおり（指定無し = 全部、一覧 = その command だけ、`''` = 入れる command 無し）。
- 置き場の生成した dir を消すと link が切れる（3.5 の実測）。

### 将来の狙い（busybox 化・sandbox。今は書くだけ）

利用者が「今の案」として示した、superbash に一括で手当てさせる理由:

1. **busybox 化**: superbash が command を用意する工程で、command 群を busybox のような 1 つの多機能の実行 file にまとめる。
2. **sandbox の中での実行**: superbash が、sandbox の中で実行する command 群を組み立てる。

どちらも、toolset の取得から command の用意までを superbash が 1 か所で持つ事でできる。実現の方式（対象の言語・build の場所・OS ごとの sandbox の仕組みの差）はまだ決めていない。

---

## 3. toolset と定義の形式

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
| `plugins[].name` | `commands[].name`（command の名前。`--allow-cmd` と道具の `name` に使う） |
| `plugins[].description` / `category` | 同じ |
| `plugins[].source` | `commands[].tool`（`tools/<name>` の `<name>`）と `tools.<name>.entry`（入口） |
| （無し） | `commands[].version`・`commands[].spec`（`subcommand`・`usage`・`in`・`out`） |

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

### 3.4 項目

- `tools.<name>.entry`: `tools/<name>/` からの相対 path の入口（package.json に bin を並べないので、ここが入口の正本）。
- `commands[].name`: command の名前。
- `commands[].tool`: `tools` の key。
- `commands[].version`: その command の版。書き手が指定する。
- `commands[].spec`:

| 項目 | 意味 |
|---|---|
| `subcommand` | 使う副 command の名前 |
| `usage` | 1 行の使い方 |
| `in` | 入力の定義。wrapper はこの規則で args（JSON）を command の普通の引数に組み立てる（形は 3.3 の提案） |
| `out` | 出力の定義（stdout の形式と説明・stderr の説明・終了 code の意味） |

- **help**: すべての command は必須の副 command `help` を持ち、`spec` の内容を出す。superbash は起動の時に `help` を実行して定義と照合しない（定義を正とする）。

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
2. **引数。** wrapper が `spec.in` の規則で args（JSON）から argv の配列を組む。
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
| `name` | command の名前（`commands[].name`） |
| `args` | command への引数（JSON）。wrapper が `spec.in` の規則で普通の引数に組み立てる |
| `timeout` | この実行の時間の上限。省略時は 10 分 |
| `run_in_background` | background で実行する |

- 道具の説明に、実行できる command の一覧（名前・`description`・`usage`・`in`・`out`）を入れる（model が `name` と `args` を書けるように）。

### 返り値

- Claude Code の普通の Bash と同じく、出力（stdout・stderr）と終了 code を返す。
- background の時は、実行の id と出力を読む先を返し、後から出力を読める。利用者の決定「普通の Bash と同じ」に従い、出力を file に書き、その path を返す。

### 失敗の形

- 実行できない `name`（toolset に無い・`--allow-cmd` に無い・`--allow-cmd ''` で起動した時）は、error として返す。
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
- 静的な検査: source に shell を通す起動（`sh -c` など）が無い事。

---

## 10. 段階

| 段 | 中身 |
|---|---|
| 第 1 段 | この素案の決定の全部（MCP の mode・`--only-install`・`--allow-cmd`・道具 `superbash-command`・wrapper・時間の上限と background・mac と Linux glibc の build） |
| 後の段 | mac の署名ビルドを toolset の command の 1 つとして載せる |
| 将来の狙い | busybox 化、sandbox の中での実行（2 章） |

---

## 11. 未決

今は無い。前の未決 2 件は利用者の決定で確定した:

- only-install の後の打ち方は `cmdA` と `npx cmdA` の 2 つ。`npx <名前> cmdA` は持たない（振り分け役の bin も作らない）。
- only-install で入れる command の範囲は `--allow-cmd` の規則をそのまま使う。
