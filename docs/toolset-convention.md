# superbash の toolset の作法

2026-11-23。確定（利用者の決定。6 章の 10 件は推奨どおりに決定）。docs/superbash-design.md（以下「設計」）の 3 章（toolset の形）・4 章（実行の規則）・6 章（道具の見せ方）に足す作法を書く。推測で書いた所は「推測」と書く。6 章は、未決だった 10 件の案と決定。

読んだ物:

| 物 | 版 | 見た所 |
|---|---|---|
| superbash | 0.1.2（main の `ba91394`） | 設計、`toolconfig.schema.json`、`src/config.rs`・`cli.rs`・`generate.rs`・`mcp/server.rs`、`wrapper/run.js` |
| superbash-bantou | 0.2.1 | `.manifest/toolconfig.json`、`tools/macbuild/`・`tools/publish/`、`tests/spec.test.js` |
| citty | 0.2.2 | README、`src/` の `command.ts`・`main.ts`・`args.ts`・`_parser.ts`・`types.ts`・`index.ts` |
| 社内の参考実装（busybox 風の multi-call dispatcher） | 実験中 | dispatcher の source と覚え書き。同じ所の他の tool の構成 |

---

## 1. 目的と範囲

### 目的

- サブコマンド（例 `github release create`）を、toolset の定義（`.manifest/toolconfig.json`）で表せるようにする。
- 道具ごとの dir（`tools/<名前>/`）の中の推奨構成と、TS で書く時の citty の書き方を決める。
- busybox 化（設計 2 章の将来の狙い）の土台を、この作法の上に置く。

### やらない事

- code を書く・直す（この文書は設計だけ。実装は superbash 0.2.0 と superbash-bantou の段で行う）。
- 道具を増やす。道具は `superbash-command` 1 つのまま。
- 任意の command 文字列・shell の構文を受ける事（設計 1 章のまま）。
- 各 command の引数の中身（呼ぶ側が何を渡すか）を決める事。決めるのは、どの要素がどの引数を持つか（引数の持ち方）だけ。
- 今の JS の command を TS に書き換える事を求める事。決めるのは「TS で作る時」の作法。
- busybox 化・sandbox の実装（土台の考え方だけ。5 章）。

---

## 2. 前提（利用者の決定）

| # | 決定 |
|---|---|
| 1 | 道具は `superbash-command` 1 つ（想定どおり）。 |
| 2 | サブコマンドは、manifest の schema で、command と同じ形の要素（name・description・usage・in・out、さらに subcommands）の配列として command に持たせる。入れ子は何段でも同じ形。args は今のまま、呼ぶ command かサブコマンドの引数だけを持つ。 |
| 3 | toolset は道具ごとの dir（`tools/<名前>/`）を守る。その下の推奨構成を決める（例 `src/index.ts` か `src/main.ts`、`src/command/<サブコマンド>.ts`。file の名前はどちらでもよい）。toolset の command を TS で作る時は citty を使う。 |
| 4 | busybox 化の土台を、この作法の上で考える（社内の参考実装は参照に留める）。 |
| 5 | command と toolset（の host の設定）は、ネットワークの宛先・アドレスを持たない。 |
| 6 | 公開の操作の command も、サブコマンドの形で組む: `npm` → `publish`、`github` → `repo create`・`release create`、`gitea` → `repo create`・`release create`。今の 6 つ（`npm-publish`・`verdaccio-publish`・`github-repo-create`・`github-release-create`・`gitea-repo-create`・`gitea-release-create`）はこの形に移す（移し方は 6.4 で決定）。 |

設計から引き継ぐ事（変えない）:

- manifest が正本。superbash は起動の時に command の `help` を実行して定義と照合しない（設計 3.4）。
- wrapper が `spec.in` の規則で args（JSON）から argv の配列を組み、command の bin を shell を通さずに起動する（設計 2・4 章）。
- `--allow-cmd` の規則（指定無し = 全部、一覧 = その物だけ、`''` = 無し）。

---

## 3. manifest の schema の拡張

### 3.1 言葉

| 言葉 | 意味 |
|---|---|
| 要素 | `commands[]` の 1 つ、またはその下の `subcommands[]` の 1 つ |
| 上の要素 | `commands[]` の直下の要素（今の command） |
| 群 | `subcommands` を持つ要素 |
| 葉 | `subcommands` を持たない要素 |
| 並びの名前 | 上の要素からその要素までの `name` を、上から順に空白で区切って並べた物（例 `github release create`）。上の要素だけなら今の名前と同じ |

### 3.2 要素の形

この作法では、`usage`・`in`・`out` を今のとおり `spec` の箱に入れたままにする（決定 2 の並びを `spec` の中に置く読み方。理由と別の案は 6.7）。

| 項目 | 上の要素 | サブコマンドの要素 | 意味 |
|---|---|---|---|
| `name` | 必須 | 必須 | 名前。サブコマンドの要素では argv の語にもなる |
| `description` | 任意 | 任意 | 説明 |
| `category` | 任意 | 無し | 今と同じ |
| `tool` | 必須 | 無し（上の要素の `tool`） | `tools` の key |
| `version` | 任意 | 無し（上の要素の版） | 今と同じ |
| `spec.subcommand` | 任意 | 無し | bin の直後に付ける語（今と同じ意味。6.7） |
| `spec.usage` | 任意 | 任意 | 1 行の使い方 |
| `spec.in` | 任意 | 任意 | その要素を呼ぶ時の引数の定義（形は今と同じ） |
| `spec.out` | 任意 | 任意 | 出力の定義（形は今と同じ） |
| `subcommands` | 任意（新） | 任意（新） | 子の要素の配列 |
| `spec` そのもの | 葉は必須、群は省ける | 同じ | 今は全 command で必須 |

今の `commands[]` との対応:

| 今 | 新しい形 |
|---|---|
| `commands[].name` | 上の要素の `name`（並びの名前の先頭） |
| `commands[].description`・`category`・`tool`・`version` | 上の要素に同じ物 |
| `commands[].spec.subcommand` | 上の要素の `spec.subcommand`（上の要素だけが持つ） |
| `commands[].spec.usage`・`in`・`out` | 各要素の `spec.usage`・`in`・`out`（葉は呼ぶ時に使う） |
| （無し） | 各要素の `subcommands` |

検査（決定（推奨どおり））:

- 同じ親の下の `name` は重ならない（今の「command の名前が重複しない」を各段に広げる）。
- `name` に空白を含めない（6.1 の決定の指し方のため）。
- `spec.subcommand` は上の要素だけ。
- 呼べるのは葉だけ（6.8）。

### 3.3 argv の組み方

```
argv = [上の要素の spec.subcommand（あれば）]
     + [上の要素の下から、呼ぶ要素までのサブコマンドの name（上から順）]
     + options（定義の順）
     + positionals（定義の順）
```

- 起動する file は今と同じく `tools/<上の要素の tool>/<entry>`。
- 子の無い上の要素では、今の組み方（`[spec.subcommand] + options + positionals`）と同じになる。
- options・positionals は、呼ぶ要素の `spec.in` だけから組む（3.5）。
- positionals の前に `--` を入れるかは 6.10。

### 3.4 例（github → release → create）

```json
{
  "$schema": "https://raw.githubusercontent.com/agent-hand/superbash/main/toolconfig.schema.json",
  "name": "example-tools",
  "description": "公開の操作",
  "tools": {
    "publish": { "entry": "dist/cli.mjs" }
  },
  "commands": [
    {
      "name": "github",
      "description": "GitHub の repo と release を作る（実際に作られる）。host の gh の認証を使う。",
      "category": "publish",
      "tool": "publish",
      "version": "0.3.0",
      "spec": { "subcommand": "github" },
      "subcommands": [
        {
          "name": "repo",
          "description": "repo の操作",
          "subcommands": [
            {
              "name": "create",
              "description": "repo を作る",
              "spec": {
                "usage": "publish github repo create <repo>",
                "in": { "positionals": [{ "name": "repo", "type": "string", "description": "owner/name" }] }
              }
            }
          ]
        },
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
                    "title": { "flag": "--title", "type": "string", "description": "題。省略時は tag" },
                    "draft": { "flag": "--draft", "type": "boolean", "description": "下書きにする" }
                  },
                  "positionals": [
                    { "name": "repo", "type": "string", "description": "owner/name" },
                    { "name": "tag", "type": "string", "description": "tag の名前" }
                  ]
                },
                "out": {
                  "stdout": { "format": "json", "description": "{ ok, url, error? }" },
                  "exitCodes": { "0": "作った", "1": "失敗", "2": "使い方の誤り" }
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

`superbash-command(name="github release create", args={"title": "v1.0.0", "draft": true, "repo": "example/app", "tag": "v1.0.0"})` で呼ばれた時、wrapper が起動する argv（bin は `tools/publish/dist/cli.mjs`）:

```
publish github release create --title v1.0.0 --draft example/app v1.0.0
```

### 3.5 args の意味

- `args` は、呼ぶ要素の `spec.in` だけで読む（決定 2）。群の `spec.in` は、子を呼ぶ時には使わない。
- 群の下のどの葉にも付けたい option は、各葉の `spec.in` に書く（citty の側は args の object を共有して書ける。4.3）。
- wrapper の検査（定義に無い key は誤り・型の検査・省略できる positional の規則）は今のまま、呼ぶ要素の `spec.in` に対して行う。

### 3.6 宛先（決定 5）

- manifest（`description`・`usage` の例を含む）、`tools/<名前>/` の code、toolset の host の設定に、ネットワークの宛先・アドレスを書かない。
- superbash は宛先を見分けて検査はしない（見分けられない）。作法として設計 3 章に書く。

### 3.7 今の schema との互換

| 場合 | 動き |
|---|---|
| 今の形の toolset を新しい superbash で読む | そのまま正しい。子の無い上の要素は葉として今と同じに動く。並びの名前は今の名前（例 `macbuild-info`）と同じ |
| 新しい形の toolset を superbash 0.1.2 で読む・群に `spec` が無い | 0.1.2 は `spec` を必須で読むので「形が不正」で止まる（`config.rs`） |
| 同じ・群に `spec` がある（`spec.subcommand` を使う時など） | 0.1.2 は `subcommands` を黙って捨てる（serde は知らない key を捨てる。schema も `additionalProperties` を閉じていない）。群が引数無しの command に見え、呼ぶと tool の側で使い方の誤りになる |

- 古い形を別扱いする処理は要らない（新しい形は今の形を含む）。
- 新しい形を使う toolset は、使える superbash の版を README に書く。移す順は 6.4（superbash を先に上げる）。

---

## 4. 道具ごとの dir の推奨構成

### 4.1 構成

```
tools/<名前>/
  src/
    main.ts                    citty の main を `export const main` で出す。実行はしない（index.ts でもよい）
    cli.ts                     実行の入口。main を読み込み、引数を渡して走らせる
    command/
      <サブコマンド>.ts         葉 1 つ
      <サブコマンド>/index.ts   群（子を subCommands に並べる）
      <サブコマンド>/<子>.ts    その子（さらに入れ子なら dir を重ねる）
    lib/                       共通の処理（任意）
  dist/
    cli.mjs                    build の結果（1 file にまとめた物）← tools.<名前>.entry が指す
```

- `main.ts`（定義）と `cli.ts`（実行）を分けるのは決定（推奨どおり）。理由: 定義を読み込んでも走らない形にしておくと、同じ定義を単独の CLI からも、busybox の振り分けからも、manifest との照合の test からも使える（5.2 の 1・2）。
- 入口の file と manifest の関係:
  - `tools.<名前>.entry` は build の結果（`dist/cli.mjs`）を指す。TS の source は指さない（6.5。node は `node_modules` の下の TS を動かさない。実測）。
  - wrapper は entry を直に起動する（`spawn(entry, argv)`）。entry の先頭に `#!/usr/bin/env node` を付け、実行の権限を付ける（今の bantou の test もこの 2 つを確かめている）。
  - 拡張子を `.mjs` にすると、toolset の `package.json` の `type` に依らず ESM として読まれる。citty 0.2.2 は ESM だけ（`type: module`、exports は `.mjs` だけ）。
- 写される範囲: superbash は `tools/<名前>/` の中だけを生成した package に写す（`generate.rs`）。`tools/<名前>/` の外の file（toolset の root の共通の lib など）は入らない。共通の code は build で `dist/` の中に取り込む。
- 依存: superbash は toolset の root の `package.json` の `dependencies` だけを写す。`tools/<名前>/package.json` の依存は入らない。citty は build で `dist/` に取り込み、`dependencies` に書かない（npx の時に registry に取りに行かない）。build の道具（citty・TypeScript・まとめる道具）は root の `devDependencies` に書く（superbash は写さない）。
- 宛先（決定 5）: `src/` にも、citty の args の `default` にも、host の設定の読み込みにも、ネットワークの宛先・アドレスを書かない。

### 4.2 manifest と citty の対応

| manifest | citty（0.2.2） | 注意 |
|---|---|---|
| 要素の `name` | `subCommands` の key・`meta.name` | 同じにする。key が argv の語 |
| `description` | `meta.description` | |
| `version`（上の要素） | main の `meta.version` | |
| `spec.usage` | 無し（`renderUsage` で作れる） | manifest に手で書く |
| `spec.in.options.<key>`（`flag`・`type`・`description`） | `args.<key>`（`type: "string"` か `"boolean"`） | flag は `--<key の kebab>`。citty は kebab と camel の両方で受ける（`--target-dir` → `args.targetDir`） |
| `type: "number"` | 無し | `string` で受け、`run` で数にする |
| `spec.in.positionals[]` | `args.<name>`（`type: "positional"`、`required` = `!optional`） | citty も args を書いた順に positional を割り当てる。manifest と同じ順に書く |
| `spec.out` | 無し | manifest だけに書く |
| `subcommands` | `subCommands` | |
| `category`・`tool` | 無し | |
| （無し） | `enum`・`alias`・`default` | manifest では `string`。選べる値・既定は description に書く |

citty 0.2.2 の注意（source で確かめた）:

- 子を持つ command に `run` を書くと、子の `run` の後に親の `run` も走る（`command.ts`）。群は `run` を持たない。
- `--no-` で始まる flag は「否定」と読まれる（`--no-timestamp` は `timestamp` を false にする）。manifest の flag が `--no-xxx` の時、citty では boolean の `xxx`（`default: true`）として受ける。
- `runMain` は、引数のどこかに `--help`・`-h` があると usage を出して終了 0 で抜ける（値の位置でも）。誤りの時は usage を stdout に出し、終了 code は全部 1。→ 入口は `runCommand` で書く（4.3・6.10）。
- 知らない option は誤りにならない（node の `util.parseArgs` を `strict: false` で使う）。wrapper は定義した option しか出さないので、superbash から呼ぶ限りは困らない。
- `CLIError`（使い方の誤り）は export されていない。見分けは `error.name === "CLIError"`。

### 4.3 書き方の例（公開の操作の tool。`github release create` の分だけ）

`src/main.ts`:

```ts
import { defineCommand } from "citty";
import { help } from "./command/help";
import { npm } from "./command/npm";
import { github } from "./command/github";
import { gitea } from "./command/gitea";

// 1 段目の key は、上の要素の spec.subcommand と、必須の help
export const main = defineCommand({
  meta: { name: "publish", version: "0.3.0", description: "公開の操作" },
  subCommands: { help, npm, github, gitea },
});
```

`src/command/github/index.ts`（群。`release/index.ts` も同じ形）:

```ts
import { defineCommand } from "citty";
import { repo } from "./repo";
import { release } from "./release";

export const github = defineCommand({
  meta: { name: "github", description: "GitHub の repo と release を作る" },
  subCommands: { repo, release }, // 群は run を持たない
});
```

`src/command/github/release/create.ts`（葉）:

```ts
import { defineCommand } from "citty";

export const create = defineCommand({
  meta: { name: "create", description: "release を作る" },
  args: {
    title: { type: "string", description: "題。省略時は tag" },
    draft: { type: "boolean", description: "下書きにする" },
    repo: { type: "positional", description: "owner/name", required: true },
    tag: { type: "positional", description: "tag の名前", required: true },
  },
  async run({ args }) {
    // host の gh を引数の配列で起動する（shell を通さない）。結果を stdout に JSON で 1 つ出す。
  },
});
```

`src/cli.ts`（入口。`runMain` は使わない）:

```ts
import { runCommand } from "citty";
import { main } from "./main";

runCommand(main, { rawArgs: process.argv.slice(2) }).catch((e: unknown) => {
  const usage = e instanceof Error && e.name === "CLIError";
  process.stderr.write(`publish: ${e instanceof Error ? e.message : String(e)}\n`);
  process.exitCode = usage ? 2 : 1; // 使い方の誤りは 2
});
```

- build: `src/cli.ts` を `dist/cli.mjs` の 1 file にまとめ（citty も中に入れる）、先頭に shebang を付け、実行の権限を付ける。まとめる道具は決めない（esbuild・tsdown など）。
- 大きい tool は、`subCommands` の値を `() => import("./command/github").then((m) => m.github)` の形にして、呼ぶ物だけ読み込める（citty の機能）。
- 3.5 の「各葉に書く共通の option」は、`const common = { … } as const` を作り、各葉の `args` に `...common` で入れる。

---

## 5. busybox 化の土台

### 5.1 考え方

- multi-call: 1 つの実行 file が、呼ばれた名前（link の名前。argv の 0 番目か 1 番目の basename）か第 1 引数で、どの tool として動くかを選ぶ（busybox の `ls -> busybox` と `busybox ls` の 2 つの形）。
- この作法では振り分けが 2 段になる:

| 段 | 何を選ぶ | 誰が選ぶ |
|---|---|---|
| 1 | どの tool（`tools/<名前>/`）か | busybox の振り分け（今は bin の名前 = tool ごとの file） |
| 2 | tool の中のどのサブコマンドか | citty の `subCommands`（argv の並び） |

- busybox 化は 1 段目を 1 つの file にまとめる事。2 段目は今の作法のまま。

### 5.2 社内の参考実装から学べる事

参考実装の形: `bin/<名前>` を振り分けの file への symlink にし、振り分けの file が link の名前で tool を選び、`tools/<名前>/src/main.ts` の `main`（citty の command の named export）を citty の `subCommands` に差し込んで走らせる。実験中で、振り分けはまだ動いていない。採るのは考え方だけ。

1. **tool の契約を「`main` を named export する（読み込んでも走らない）」に揃える。** 振り分けの側は tool の中を知らずに読み込める。→ 4.1 の `main.ts`。
2. **定義と実行を分ける。** 参考実装の tool は `main.ts`（定義）と `cli.ts`（`runMain` を呼ぶ）を分けている。同じ定義を単独の CLI からも振り分けからも使える。
3. **`subCommands` を Proxy で動的に見せない。** 参考実装は Proxy で 1 つだけ見せる形を取り、citty の中の `await` が Proxy の `then` を引いて thenable と誤る罠、`Object.keys` が空になる罠に当たった。citty は `subCommands` の値に「読み込む関数」を許すので、名前の一覧を先に作れる時（superbash は manifest の `tools` で分かる）は普通の object で足りる。
4. **実行の時に TS を読み込まない。** 参考実装は TS を実行の時に読む loader（jiti）を使い、絶対 path の動的 import が解決しない事があった（未解決）。→ 配る前に JS にまとめる（6.5）。
5. 同じ所の他の tool は、TS を 1 つの実行 file（node の single executable application）に build している（node 25.7 以上が要ると書いてある）。OS・CPU ごとの build が要る形。

### 5.3 superbash の生成する package の bin との関係（決定（推奨どおり））

今:

| 物 | 中身 |
|---|---|
| `package.json` の `bin` | 実行できる command が使う tool ごとに `"<tool>": "tools/<tool>/<entry>"`、MCP の mode では `"superbash-run": "wrapper/run.js"` |
| MCP の mode の 1 回の呼び出し | npx → wrapper（node）→ tool の entry（node）。wrapper は entry の path を直に起動する（bin の名前を通らない） |

busybox 化の土台:

- 各 tool は、`dist/` に 2 つの物を出す: 実行の入口（今の entry）と、読み込んでも走らない `main` の module（例 `dist/main.mjs`）。manifest に module の path を足す（例 `tools.<名前>.module`）。
- superbash は、生成の時に小さな振り分けの file を置く（wrapper と同じく superbash に埋め込んだ雛形。まとめる道具は要らない）。中身は `subCommands: { <tool>: () => import("../tools/<tool>/<module>").then((m) => m.main) }` の普通の object（5.2 の 3）。
- 振り分けの file は 2 つの選び方を持つ: link の名前（only-install。npm の `bin` は複数の名前を同じ file に向けられる）と第 1 引数（MCP の wrapper。今の wrapper は bin の名前を通らないため）。
- MCP の mode では、wrapper が子の process を起こさず、同じ process で module を読み込んで走らせる形もできる（node の起動が 1 回減る）。代わりに、終了 code・stdout・時間切れの扱いを wrapper が持つ事になる。
- citty を tool ごとに `dist/` に入れると、同じ process に tool の数だけ citty が入る（動きは困らない）。1 つにするなら、振り分けの file の側で citty を持ち、tool の module は citty を外に出す build にする。
- 1 つの実行 file（single executable application）にする形は、OS・CPU ごとの build が要り、「手元で生成して npx で動かす」今の形から離れる。将来の狙い（設計 2 章）の範囲で、今回は決めない。
- sandbox は範囲外。

---

## 6. 決めた事（未決だった 10 件）

各項は「案（利点・欠点）」と「決定と理由」。10 件とも推奨どおりに決定した（利用者、2026-11-23）。

### 6.1 session がどのサブコマンドを呼ぶかの指し方

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) `name` に並びの名前（空白で区切る。`github release create`） | 打つ形（`publish github release create`）に近く、model が書きやすい。道具の引数を変えない。今の名前もそのまま 1 語の並びとして通る | `name` に空白を入れられない。`--allow-cmd` で shell・plist の quote が要る |
| (b) 記号で区切る（`github.release.create`・`github/release/create`） | 1 語で quote が要らない | 区切りの記号を名前に使えない。打つ形と違う |
| (c) `name` は上の要素、別の引数（例 `path: ["release", "create"]`）で下を指す | 形がはっきりする | 道具の引数が変わる。`--allow-cmd` には別の書き方が要る。model が 2 か所に分けて書く |
| (d) 葉ごとに平らな名前を manifest に書く（今の `github-release-create`） | 今と同じ | 入れ子の意味が薄い。名前を 2 重に持つ |

**決定: (a)。** 道具の形（決定 1・2）を変えずに済み、打つ形と並びが一致する。読み方は決定（推奨どおり）: 前後の空白は捨て、続いた空白は 1 つの区切りとして読む。

### 6.2 manifest（正本）と citty の定義の関係

同じ情報（名前・説明・引数）が manifest と citty の定義の 2 か所に書かれる。

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) 両方を手で書き、toolset の test で照合する（今の bantou の形: `lib/commands.js` と manifest を `tests/spec.test.js` で照合） | 新しい道具が要らない。manifest は手で読める正本のまま。`out` など citty に無い情報の置き場で悩まない | 2 か所を直す手間。照合の test を書く手間 |
| (b) citty の定義から manifest を作る（build の時に `main` を読み込み、`meta`・`args`・`subCommands` を辿って書き出し、commit する） | 書くのは 1 か所 | `out`・`usage`・`category`・`number` の型など citty に無い情報の置き場を別に決める。作る道具（node の script）を誰が持つかを決める |
| (c) manifest から citty の `args` を作る（実行の時に manifest を読み、`args`・`meta` を組む小さな helper） | manifest が正本のままで 1 か所 | `args` の TS の型が付かない。helper をどこに置くかを決める。`--no-` の否定などの写し方を helper が持つ |
| (d) 実行の時に `help` を読む（superbash が起動の時に tool の `help` を実行して定義を得る） | toolset に manifest の command の定義が要らない | 設計 3.4 の決定（help を実行して照合しない）に反する。起動の時に toolset の code を走らせる。manifest を見ただけでは中身が分からない |

**決定: 第 1 段は (a)。** 照合は toolset の test で、`main` を読み込んで `subCommands` を辿り、名前・説明・引数（4.2 の対応）を manifest と比べる。理由: 正本の位置づけと設計 3.4 の決定をそのまま保てる。今の bantou と同じやり方で、新しい道具が要らない。command の数が増えて 2 か所の手間が重いと分かった時に (b) を足す（その時の照合は「作り直して差分が無い」に変わる）。

### 6.3 道具の説明に並べる量が増える事

量の目安（推定）: 今の bantou の 9 command で、道具の説明は約 11,600 字（`server.rs` の説明の組み方を写して数えた）。内訳は `out`（JSON のまま）約 4,600 字、`in` 約 2,800 字、`description` 約 2,600 字。macbuild の 3 command だけを出した superbash に Claude Code から繋いだ時、説明（約 4,150 字）は切られずに届き、道具は遅延読み込み（使う前に説明を読み込む形）で出ていた（この Mac で確かめた）。説明の長さに上限があるかは未確認。サブコマンドで葉が増えると、量は葉の数に比例して増える。

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) 今のまま全部並べる | 1 回で全部分かる | 葉の数に比例して増える |
| (b) 木の形で並べ、葉は `name`・`description`・`usage`・`in` を全部、`out` は短く（`stdout.format` と終了 code の意味だけ。`stdout.description` の長文は外す） | 引数を書くのに要る物は全部残る。群の説明は 1 回で済む。道具の引数を変えない | `stdout` の JSON の形の説明が見えなくなる（出力そのものは返る） |
| (c) 一覧は名前と `description` だけにし、詳しい物は別の口で引く（例 予約の名前で spec を返す） | 量が最も小さい | 呼ぶ前に 1 往復増える。引く口（予約の名前か道具の引数）を足す |
| (d) `--allow-cmd` で絞る（運用。今もできる） | 変える物が無い | 量の問題そのものは残る。起動の設定の手間 |

**決定: (b) と (d)。** 理由: model が `args` を書くのに要るのは `in` で、`out` の長文は呼んだ後の出力で補える。今の量の約 4 割（`out`）の大半を削れる。道具の形も変えない。`--allow-cmd` は並びの名前・群の名前で絞れるようにする（6.6）と、群の単位で説明を減らせる。葉が数十を超えて (b) でも重い時に (c) を考える。

### 6.4 今の command（macbuild の 3 つ、公開の操作の 6 つ）の移し方

名前の写し（決定 6 と、macbuild の同じ組み方。tool の dir は今のまま）:

| 今の名前 | 新しい並びの名前 | tool | argv（bin の後） |
|---|---|---|---|
| `macbuild-info` | `macbuild info` | macbuild | `info`（今と同じ） |
| `macbuild-tauri` | `macbuild tauri` | macbuild | `tauri …`（今と同じ） |
| `macbuild-sign` | `macbuild sign` | macbuild | `sign …`（今と同じ） |
| `npm-publish` | `npm publish` | publish | `npm publish …` |
| `verdaccio-publish` | 無くなる（`npm publish` に寄せる） | | |
| `github-repo-create` | `github repo create` | publish | `github repo create …` |
| `github-release-create` | `github release create` | publish | `github release create …` |
| `gitea-repo-create` | `gitea repo create` | publish | `gitea repo create …` |
| `gitea-release-create` | `gitea release create` | publish | `gitea release create …` |

- macbuild は上の要素 `macbuild`（`spec.subcommand` 無し）の下に `info`・`tauri`・`sign`。argv は今と同じなので、変わるのは manifest（と `help` の出力）だけ。
- 公開の操作は、1 つの tool（`publish`）の下に上の要素 `npm`・`github`・`gitea` を並べ、各々の `spec.subcommand` に同じ語を書く（6.7 の (i)）。tool の dir を `npm` などにしないのは、only-install で npm の global の bin に `npm` という名前の link を作る事になり、host の `npm` とぶつかるため。

決定 5 に照らした今の code の事実（移す時に外す物）:

- host の設定の `verdaccioRegistry`・`giteaUrl`・`giteaUsername`（設定の file の key と、環境変数 `PUBLISH_VERDACCIO_REGISTRY`・`PUBLISH_GITEA_URL`・`PUBLISH_GITEA_USERNAME`）。`tools/publish/lib/config.js` の読み込みも外す。
- `verdaccio-publish` は名前が宛先。
- `tools/publish/lib/npm.js` は npmjs の registry の URL を定数で持つ（`npm-publish` の宛先）。
- `tools/publish/lib/github.js` は、gh の出力に URL が無い時、`https://github.com/<repo>` を組んで結果に入れる（表示の為の URL）。決定 5 に当たる（利用者、2026-11-23）。gh が返した URL だけを出し、自分で組まない。

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) bantou の 1 つの版で、新しい名前だけにする（古い名前は残さない） | 名前が 1 組で、説明が増えない | 呼ぶ側の覚え・起動の `--allow-cmd` を同時に直す |
| (b) 古い名前を新しい名前と並べて残す期間を置く | 呼ぶ側が少しずつ移れる | 説明が 2 重になる（6.3）。`verdaccio-publish` は決定 5 で残せない |
| (c) 段に分ける: 先に superbash を新しい形に対応させ（今の形も読める）、次に bantou の manifest と CLI を移し（JS のまま）、TS・citty への書き換えは tool ごとに後で行う | 一度に変わる物が少ない。どの段も前の段だけに依る | 段の数が増える |

**決定: (c) の順で、bantou の段は (a)。** 理由: 決定 5 で `verdaccio-publish` と host の設定が変わり、公開の操作は名前を残しても互換にならない。使う所が少なく、2 重の説明の害の方が大きい。TS への書き換えは決定 3 で求められていない（「TS で作る時」）ので、名前・形の移しと分ける。

移す時の注意:

- 起動の `--allow-cmd` に古い名前が残ると、superbash は警告を出すだけで止めず（`cli.rs`）、その command 無しで起動する。bantou を移す時に、MCP の設定・launchd の plist の `--allow-cmd` も同時に直す。
- bantou の `package.json` の `version` を上げる（npx が前の写しを使い続けるため。bantou の README の既知の事）。

### 6.5 TS の command をどう動かすか

事実（この Mac で確かめた）:

- node 26.8.2 は、`node_modules` の外の `.ts` を型を除いてそのまま動かした。`node_modules` の下の `.ts` は `ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING` で止まった。
- MCP の mode の生成した package は、npm の cache の `_npx/<hash>/node_modules/<package>/` の下で動く（この Mac の npm の cache で確かめた）。→ entry が `.ts` だと MCP の mode で動かない。
- node 18 は型を除けない（bantou の `engines` は node 18 以上）。
- citty 0.2.2 は ESM だけ。

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) TS をそのまま entry にする（node の型の除去） | build が要らない | MCP の mode で動かない（上の実測）。node の版に依る |
| (b) entry の JS から、実行の時に TS を読む loader（jiti など）で読み込む | build が要らない | loader を `dependencies` に入れる（npx の時に registry に取りに行く）。起動が遅くなる（推測）。参考実装で動的 import が解決しない事があった |
| (c) 配る前に JS の 1 file にまとめる（citty も中に入れる） | node 18 でも動く形にできる。依存が無い。`tools/<名前>/` の外の共通 code も取り込める | build の段が要る。git の repo で指す時は `dist/` の置き方を決める |
| (d) 1 つの実行 file（single executable application） | node が要らない形にできる | OS・CPU ごとの build。build に新しい node が要る。大きい |

**決定: (c)。** 理由: superbash の今の流れ（取得 → 写す → npx）を変えずに動き、npx の時に registry に行かない。

`dist/` の置き方（(c) の中の小さな決定）:

| 案 | 利点 | 欠点 |
|---|---|---|
| repo に `dist/` を置き、CI で「build し直して差分が無い」を確かめる | 取得の経路（registry・git）に依らず動く | 生成物を repo に持つ |
| registry に出す時は `prepack`、git で指す時は `prepare` で build する | repo に生成物を持たない | `npm pack <git の指定>` で `prepare` が走り、build の道具が入るかは未確認（設計 3.5 の (5) は npx に git を直に渡した時の確かめ） |

決定: 当面は repo に置く（bantou は git の repo の名前で指す使い方があるため）。

### 6.6 `--allow-cmd` でサブコマンドを指せるか

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) 上の要素の名前だけ（下は全部） | 単純 | 葉の単位で絞れない（例 `github repo create` だけ出したい時） |
| (b) 並びの名前を書ける。群の並び（`github`・`github release`）はその下の全部、葉の並びはその葉だけ | 群でも葉でも絞れる。6.1 (a) と同じ書き方 | 名前の中の空白の quote（`--allow-cmd 'macbuild info,github'`） |
| (c) 葉の並びだけ | 絞りが明示的 | 群で書けず長くなる |

**決定: (b)。** 理由: 6.1 と書き方が揃い、6.3 の説明の量を群の単位で減らせる。

注意:

- only-install の mode では、入れる単位は tool の bin。葉で絞っても bin は tool 全体を入れ、打てば全部のサブコマンドが動く（今も、1 つの tool の中の別の command は bin を打てば動く）。絞りが効くのは「どの tool の bin を入れるか」まで。
- 知らない名前は、今のとおり警告だけにする（6.4 の注意も参照）。

### 6.7 `usage`・`in`・`out` の置き場と `spec.subcommand` の扱い

置き場:

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) `spec` の箱を残す（この文書の形） | 今の形が 1 字も変わらず正しい。superbash の読む形が 1 つ。`spec.subcommand` の居場所が今のまま | 1 段深い。決定 2 の並び（name・description・usage・in・out）と見た目が違う |
| (b) 要素の直下に出す（`spec` を無くす） | 決定 2 の並びどおり。浅い | 古い形と新しい形の 2 つを読む（か、全部を移す）。`subcommand`（語）と `subcommands`（配列）が同じ段に並んで紛れる |

`spec.subcommand`（bin の直後に付ける語）:

| 案 | 利点 | 欠点 |
|---|---|---|
| (i) 上の要素だけに残す | 1 つの tool に上の要素を並べられる（6.4 の `publish` の下の `npm`・`github`・`gitea`）。今の意味のまま | 上の要素の `name` と argv の語が別になりうる |
| (ii) 無くす（1 つの tool = 1 つの上の要素） | 名前と argv が一致する | 公開の操作を tool の dir に分けると、bin の名前が host の command（`npm`）とぶつかる。共通の code（伏せ・子の process）を tool ごとに持つ |

**決定: (a) と (i)。** 理由: 互換を保つ処理が要らず、公開の操作を 1 つの tool に置ける。

### 6.8 子を持つ要素（群）を呼べるか

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) 葉だけ呼べる | citty の形（群は `run` を持たない）と合う。説明の一覧も葉だけで済む | 群に「既定の動き」を持たせられない |
| (b) `spec.in` を書いた群も呼べる | 群に既定の動きを持たせられる | citty では親の `run` が子の後にも走るので、書き方の注意が要る。呼ぶ時に「どの子か」が無い |

**決定: (a)。** 群を呼ばれたら、wrapper は「子を指して」と子の名前の一覧を誤りで返す。

### 6.9 必須の `help` の置き場（入れ子の時）

設計 3.4: すべての command は必須の副 command `help` を持ち、spec の内容を出す。

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) bin の直後だけ: `<bin> help [<並び>]`（例 `publish help github release create`） | 今の bantou の形（`macbuild help macbuild-tauri`）と同じ 1 か所 | bin の直後の語として `help` を予約する（上の要素の `spec.subcommand` と、`spec.subcommand` の無い上の要素の 1 段目の子の名前に使えない） |
| (b) 各段に `help` を置く（`publish github help`） | 群ごとに引ける | 全部の群に置く手間。各段で `help` を予約する |
| (c) citty の `--help` に任せる | 書かなくてよい | 出るのは usage の文で、spec（`in`・`out`）ではない。入口で `runMain` を使わない（6.10）と出ない |

**決定: (a)。** 引数は並びの名前（`spec.subcommand` の語からでも、上の要素の `name` からでもよい）。

### 6.10 wrapper の argv の組み方（`--`）と入口の雛形

事実:

- 今の wrapper は positionals の前に `--` を入れない。`-` で始まる値は option と読まれる（bantou の既知の制限）。
- citty の `runMain` は引数のどこかに `--help`・`-h` があると、何もせず usage を出して終了 0（値の位置でも）。
- option の値が `-` で始まる時: bantou の CLI は次の 1 つをそのまま値に取る。citty が使う node の `util.parseArgs`（`strict: false`）も値として読む（node 26.8.2 で `--tag -x -- -h b` が `tag: "-x"`、positionals `["-h", "b"]` になった）。

| 案 | 利点 | 欠点 |
|---|---|---|
| (a) wrapper が positionals の前に `--` を入れる。TS の入口は `runCommand` で書く（4.3） | `-` で始まる positional を渡せる。bantou の CLI（`--` を読む）も citty（`util.parseArgs`）も受ける | `--` を受けない CLI の toolset が動かなくなる |
| (b) 今のまま（既知の制限として書く） | 変える物が無い | 制限が残る |
| (c) option を `--flag=value` の形で渡す | 値の `-` の紛れが無い | 今の bantou の CLI は `=` の形を受けない |

**決定: (a)。** 理由: 今ある toolset（bantou）が両方受け、制限が 1 つ消える。入口を `runCommand` にするのは、`-h` の紛れと、使い方の誤りの終了 code（2）と stdout の汚れ（usage）を避けるため。

---

## 7. この作法で変わる superbash 側の物

| 物 | 変わる事 | 関係 |
|---|---|---|
| `toolconfig.schema.json` | `command` に `subcommands` を足す。サブコマンドの要素の定義（再帰）を足す。群では `spec` を省ける。`spec.subcommand` は上の要素だけ。`name` に空白を含めない | 3.2・6.7 |
| `src/config.rs` | 再帰の読み込み。兄弟の名前の重複・`help` の予約の検査。葉の一覧（並びの名前・tool・argv の頭）を作る | 3.2・6.9 |
| `wrapper/run.js`（argv の組み方） | 並びの名前で葉を探す（群なら子の一覧を誤りで返す）。argv に `spec.subcommand` と並びの名前を足す。positionals の前の `--` | 3.3・6.8・6.10 |
| `src/mcp/server.rs`（道具の説明・照合） | 説明を木の形で作る（量の扱い）。`name` を並びの名前で照合する。input schema の `name` の説明（「サブコマンドは空白で区切った並び」） | 6.1・6.3 |
| `src/cli.rs`（`--allow-cmd`） | 並びの名前・群の名前を受ける。知らない名前の警告を並びの名前で | 6.6 |
| `src/generate.rs` | 今は変えない（bin は tool ごと）。busybox 化の時に振り分けの file を置く | 5.3 |
| `docs/superbash-design.md` | 3 章（要素の形・argv・宛先の作法・道具ごとの dir の推奨構成。この文書を参照）、4 章（`--`）、6 章（説明・`name`） | 全体 |
| `README.md`・test | 入れ子の fixture（3 段）。`--allow-cmd` の群・葉。古い形の fixture がそのまま通る事 | 3.7 |

- 決定 5 について superbash 自身が変える物は無い（superbash は今も宛先を持たない）。作法として設計 3 章に書く（3.6）。
- busybox 化の時に足す物（今回は決めない）: `tools.<名前>.module`、振り分けの file の雛形、wrapper の同じ process での実行（5.3）。

---
